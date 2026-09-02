#!/usr/bin/env bash
# One command that does the GitHub key dance, so nobody has to reread the docs.
#
# GitHub treats these as three separate things, which is the part that is easy
# to forget:
#   1. an ssh AUTHENTICATION key   (git push)          scope: admin:public_key
#   2. an ssh SIGNING key          (Verified badge)    scope: admin:ssh_signing_key
#   3. a gpg key                   (Verified badge)    scope: write:gpg_key
# The same ssh key must be uploaded twice, once as each type.

gh_need() {
  has gh || { log_err "gh is not installed (brew install gh)"; return 1; }
  gh auth status >/dev/null 2>&1 && return 0
  log_warn "gh is not authenticated"
  confirm "Run 'gh auth login' now?" || return 1
  gh auth login || return 1
}

# gh_scope <scope> — make sure the token carries a scope, refreshing if not.
gh_scope() {
  local want="$1"
  local have; have="$(gh auth status 2>&1 | sed -n "s/.*Token scopes: //p" | tr -d "'")"
  case ",${have//, /,}," in
    *",$want,"*) return 0 ;;
  esac
  log_warn "your gh token is missing the '$want' scope"
  confirm "Refresh the token to add '$want'? (opens a browser)" || return 1
  gh auth refresh -h github.com -s "$want"
}

gh_key_title() { printf '%s' "${DOTFILES_KEY_TITLE:-$(scutil --get ComputerName 2>/dev/null || hostname -s)}"; }

gh_add_ssh_auth() {
  local pub="$1"
  local body; body="$(awk '{print $2}' "$pub")"
  if gh ssh-key list 2>/dev/null | grep -qF "$body"; then
    log_skip "ssh authentication key already on github"
    return 0
  fi
  gh_scope admin:public_key || { log_warn "skipping auth key upload"; return 0; }
  confirm "Upload $(pretty_path "$pub") as an AUTHENTICATION key?" || { log_skip "declined"; return 0; }
  gh ssh-key add "$pub" --title "$(gh_key_title)" \
    && log_ok "uploaded ssh authentication key" \
    || log_warn "upload failed"
}

gh_add_ssh_signing() {
  local pub="$1"
  local body; body="$(awk '{print $2}' "$pub")"
  # Listing signing keys needs its own scope, so a failure here is not proof
  # the key is absent -- only that we cannot see it.
  local listed; listed="$(gh ssh-key list --type signing 2>/dev/null || true)"
  if [[ -n "$listed" ]] && grep -qF "$body" <<< "$listed"; then
    log_skip "ssh signing key already on github"
    return 0
  fi
  gh_scope admin:ssh_signing_key || { log_warn "skipping signing key upload"; return 0; }
  listed="$(gh ssh-key list --type signing 2>/dev/null || true)"
  if [[ -n "$listed" ]] && grep -qF "$body" <<< "$listed"; then
    log_skip "ssh signing key already on github"
    return 0
  fi
  confirm "Upload the SAME key again as a SIGNING key? (github treats these separately)" \
    || { log_skip "declined"; return 0; }
  gh ssh-key add "$pub" --type signing --title "$(gh_key_title) (signing)" \
    && log_ok "uploaded ssh signing key" \
    || log_warn "upload failed"
}

gh_add_gpg() {
  local key="$1"
  has gpg || return 0
  gpg --list-secret-keys "$key" >/dev/null 2>&1 || { log_skip "gpg key $key not in this keyring"; return 0; }

  local listed; listed="$(gh gpg-key list 2>/dev/null || true)"
  if [[ -n "$listed" ]] && grep -qiF "${key: -16}" <<< "$listed"; then
    log_skip "gpg key already on github"
    return 0
  fi
  gh_scope write:gpg_key || { log_warn "skipping gpg key upload"; return 0; }
  confirm "Upload gpg public key $key to github?" || { log_skip "declined"; return 0; }

  local tmp; tmp="$(mktemp)"
  gpg --armor --export "$key" > "$tmp"
  gh gpg-key add "$tmp" \
    && log_ok "uploaded gpg public key" \
    || log_warn "upload failed (already present?)"
  rm -f "$tmp"
}

gh_verify() {
  log_head "Verifying"

  local probe; probe="$(ssh -o BatchMode=yes -o ConnectTimeout=10 -T git@github.com 2>&1 || true)"
  case "$probe" in
    *"successfully authenticated"*) log_ok "ssh: $(sed 's/^Hi //;s/!.*//' <<< "$probe" | head -1) authenticated" ;;
    *"Permission denied"*)          log_err "ssh: github rejected the key" ;;
    *)                              log_warn "ssh: inconclusive — $(head -1 <<< "$probe")" ;;
  esac

  # A real signed commit in a throwaway repo is the only honest check that
  # signing is wired up end to end.
  local tmp; tmp="$(mktemp -d)"
  (
    cd "$tmp" || exit 1
    git init -q .
    git commit -q --allow-empty -m "signing probe" 2>/dev/null
  )
  local sig; sig="$(git -C "$tmp" log -1 --format='%G?' 2>/dev/null || echo '?')"
  case "$sig" in
    G) log_ok "commit signing works and verifies locally ($(git -C "$tmp" log -1 --format='%GS'))" ;;
    U) log_warn "commits are signed but the key is untrusted locally" ;;
    N) log_warn "commits are NOT signed — run: dotfiles install" ;;
    *) log_warn "could not determine signing status ($sig)" ;;
  esac
  rm -rf "$tmp"
}

cmd_github() {
  local ssh_key="${CORE_SSH_KEY:-$HOME/.ssh/id_ed25519}"
  local gpg_key="${CORE_GPG_KEY:-79007EFD4678CB94}"

  log_head "GitHub key setup"
  gh_need || return 1
  log_ok "authenticated as $(gh api user --jq .login 2>/dev/null || echo '?')"

  if [[ ! -f "$ssh_key.pub" ]]; then
    log_err "no ssh key at $(pretty_path "$ssh_key") — run: dotfiles install"
    return 1
  fi

  log_head "SSH key: $(pretty_path "$ssh_key")"
  gh_add_ssh_auth "$ssh_key.pub"

  # Only offer the signing upload when git is actually set to sign with ssh.
  if [[ "$(git config --global gpg.format || echo openpgp)" == "ssh" ]]; then
    gh_add_ssh_signing "$ssh_key.pub"
  else
    log_skip "git signs with gpg, not ssh — skipping ssh signing key"
  fi

  log_head "GPG key: $gpg_key"
  gh_add_gpg "$gpg_key"

  gh_verify
  printf '\n'
}
