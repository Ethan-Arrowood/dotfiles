#!/usr/bin/env bash
# Portable, encrypted backup of the GPG signing key.
#
# The point of the extra encryption layer: a bare `gpg --export-secret-keys`
# is protected only by the key's own passphrase, so anywhere you put it has to
# be treated as a secret store. Wrapped with age (or gpg --symmetric), the
# artifact is a useless blob without a second passphrase, which makes it safe
# to keep somewhere merely convenient -- a private repo, a gist, cloud storage.
#
# It is still not safe to put the UNWRAPPED key in any of those places.

key_cipher() {
  if has age; then printf 'age'
  elif has gpg; then printf 'gpg'
  else return 1; fi
}

key_usage() {
  cat <<'HELPTEXT'
dotfiles key — move the signing key between machines, safely

  dotfiles key export [-o FILE]   encrypt this machine's gpg secret key
  dotfiles key import FILE        decrypt and import it on a new machine
  dotfiles key status             what signing material this machine has

The exported file is encrypted with a passphrase you choose, on top of the
passphrase already on the key itself. Store it wherever is convenient.

  # old machine
  dotfiles key export -o ~/Dropbox/gpg-signing-key.age

  # new machine
  dotfiles key import ~/Dropbox/gpg-signing-key.age
  dotfiles install --profile mac-dev

Do NOT store an unencrypted `gpg --export-secret-keys` dump in a git repo or
gist. A "secret" gist is unlisted, not private: anyone with the URL can read
it, no login required. A private repo is readable by every token with `repo`
scope — CI, OAuth apps, agents — and git history keeps it after deletion.
HELPTEXT
}

key_status() {
  log_head "Signing material on this machine"
  local gpg_key="${CORE_GPG_KEY:-79007EFD4678CB94}"
  local ssh_key="${CORE_SSH_KEY:-$HOME/.ssh/id_ed25519}"

  if has gpg && gpg --list-secret-keys "$gpg_key" >/dev/null 2>&1; then
    log_ok "gpg secret key $gpg_key present"
    gpg --list-secret-keys --keyid-format=long "$gpg_key" 2>/dev/null | sed 's/^/       /'
  else
    log_skip "gpg secret key $gpg_key not in this keyring"
  fi

  if [[ -f "$ssh_key" ]]; then
    log_ok "ssh key $(pretty_path "$ssh_key")"
    ssh-keygen -lf "$ssh_key.pub" 2>/dev/null | sed 's/^/       /'
  else
    log_skip "no ssh key at $(pretty_path "$ssh_key")"
  fi

  local cipher; cipher="$(key_cipher)" \
    && log_ok "export/import available (using $cipher)" \
    || log_warn "neither age nor gpg installed; export/import unavailable"
  printf '\n'
}

key_export() {
  local out="" gpg_key="${CORE_GPG_KEY:-79007EFD4678CB94}"
  while [[ $# -gt 0 ]]; do
    case "$1" in
      -o|--out) out="${2:?-o needs a path}"; shift 2 ;;
      *) log_die "unknown option: $1" ;;
    esac
  done

  has gpg || log_die "gpg is not installed"
  gpg --list-secret-keys "$gpg_key" >/dev/null 2>&1 \
    || log_die "gpg secret key $gpg_key is not in this keyring — nothing to export"

  local cipher; cipher="$(key_cipher)" || log_die "need age or gpg to encrypt the export"
  [[ -z "$out" ]] && out="$HOME/gpg-signing-key-$(date +%Y%m%d).$([[ "$cipher" == age ]] && echo age || echo gpg)"
  [[ -e "$out" ]] && log_die "$out already exists; pick another path"

  log_head "Exporting $gpg_key"
  printf '  This asks for TWO passphrases:\n'
  printf '    1. the key'\''s own passphrase, to unlock it for export\n'
  printf '    2. a new passphrase to encrypt the file with\n\n'

  local old_umask; old_umask="$(umask)"
  umask 077
  local tmp; tmp="$(mktemp)"
  # shellcheck disable=SC2064
  trap "rm -f '$tmp'; umask $old_umask" RETURN

  gpg --export-secret-keys --armor "$gpg_key" > "$tmp" || log_die "gpg export failed"
  [[ -s "$tmp" ]] || log_die "gpg produced an empty export"

  case "$cipher" in
    age) age --passphrase --output "$out" "$tmp" || log_die "age encryption failed" ;;
    gpg) gpg --symmetric --cipher-algo AES256 --output "$out" "$tmp" || log_die "gpg encryption failed" ;;
  esac
  chmod 600 "$out"

  log_ok "wrote $(pretty_path "$out") ($(wc -c < "$out" | tr -d ' ') bytes, $cipher)"
  printf '\n  Safe to store in a private repo, gist, or cloud storage: without the\n'
  printf '  passphrase it is an opaque blob. Keep that passphrase in your password\n'
  printf '  manager, NOT next to the file.\n\n'
}

key_import() {
  local file="${1:?usage: dotfiles key import <file>}"
  [[ -f "$file" ]] || log_die "no such file: $file"
  has gpg || log_die "gpg is not installed"

  local cipher
  case "$file" in
    *.age) cipher=age ;;
    *)     cipher=gpg ;;
  esac
  [[ "$cipher" == age ]] && ! has age && log_die "$file needs age to decrypt; brew install age"

  log_head "Importing from $(pretty_path "$file")"

  local old_umask; old_umask="$(umask)"
  umask 077
  local tmp; tmp="$(mktemp)"
  # shellcheck disable=SC2064
  trap "rm -f '$tmp'; umask $old_umask" RETURN

  case "$cipher" in
    age) age --decrypt --output "$tmp" "$file" || log_die "age decryption failed" ;;
    gpg) gpg --decrypt --output "$tmp" "$file" || log_die "gpg decryption failed" ;;
  esac

  gpg --import "$tmp" || log_die "gpg import failed"

  local gpg_key="${CORE_GPG_KEY:-79007EFD4678CB94}"
  # An imported key carries no owner trust, and git refuses to sign with a key
  # it does not ultimately trust.
  if gpg --list-secret-keys "$gpg_key" >/dev/null 2>&1; then
    printf '%s\n' "trust
5
y
quit" | gpg --command-fd 0 --edit-key "$gpg_key" >/dev/null 2>&1 \
      && log_ok "set ultimate owner trust on $gpg_key" \
      || log_warn "could not set owner trust; run: gpg --edit-key $gpg_key trust"
  fi

  log_ok "imported"
  printf '\n  Now re-run the installer so git picks it up:\n    dotfiles install\n\n'
}

cmd_key() {
  local sub="${1:-status}"; shift || true
  case "$sub" in
    export) key_export "$@" ;;
    import) key_import "$@" ;;
    status) key_status ;;
    -h|--help|help) key_usage ;;
    *) log_err "unknown subcommand: $sub"; printf '\n'; key_usage; return 1 ;;
  esac
}
