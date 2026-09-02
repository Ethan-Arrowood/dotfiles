#!/usr/bin/env bash
# Core layer: make zsh, git and ssh usable anywhere.
#
# Sourced by bin/dotfiles with $LAYER_DIR set. Must be idempotent.

# Public identifiers only. Safe to commit: a key ID and an email are not key
# material. Private keys never live in this repo -- see `dotfiles key --help`.
CORE_GPG_KEY="${CORE_GPG_KEY:-79007EFD4678CB94}"
CORE_EMAIL="${CORE_EMAIL:-ethan@arrowood.dev}"
CORE_SSH_KEY="${CORE_SSH_KEY:-$HOME/.ssh/id_ed25519}"

# auto | gpg | ssh | off. auto prefers gpg when its key is in the keyring,
# otherwise falls back to the ssh key, so a fresh machine signs immediately
# without anyone having to move a gpg private key onto it.
DOTFILES_SIGNING="${DOTFILES_SIGNING:-auto}"

core_install_packages() {
  local missing=()
  has zsh || missing+=(zsh)
  has git || missing+=(git)
  if [[ ${#missing[@]} -eq 0 ]]; then
    log_skip "zsh and git already installed"
    return 0
  fi
  log_step "installing: ${missing[*]}"
  sys_install "${missing[@]}" || log_warn "could not install ${missing[*]} automatically"
  return 0
}

# --- ssh --------------------------------------------------------------------

core_ssh_config() {
  local cfg="$HOME/.ssh/config" inc="Include ~/.ssh/config.d/*"

  if [[ "${DOTFILES_DRY_RUN:-0}" != "1" ]]; then
    mkdir -p "$HOME/.ssh/config.d"
    chmod 700 "$HOME/.ssh" "$HOME/.ssh/config.d"
  fi

  local f
  for f in "$LAYER_DIR"/ssh/*.conf; do
    [[ -r "$f" ]] || continue
    link_file "$f" "$HOME/.ssh/config.d/$(basename "$f")"
  done

  # ssh applies the FIRST value it sees for a keyword, so the Include has to
  # come before anything already in the file for these settings to win.
  if [[ ! -f "$cfg" ]]; then
    write_file "$cfg" <<< "$inc"
    [[ "${DOTFILES_DRY_RUN:-0}" != "1" ]] && chmod 600 "$cfg"
  elif grep -qF 'config.d/' "$cfg"; then
    log_skip "~/.ssh/config already includes config.d/"
  elif [[ "${DOTFILES_DRY_RUN:-0}" == "1" ]]; then
    printf '%s  dry prepend %s to ~/.ssh/config%s\n' "$_C_DIM" "$inc" "$_C_RESET"
  else
    mkdir -p "$DOTFILES_BACKUP_DIR"
    cp "$cfg" "$DOTFILES_BACKUP_DIR/ssh_config"
    printf '%s\n\n%s' "$inc" "$(cat "$cfg")" > "$cfg.tmp" && mv "$cfg.tmp" "$cfg"
    chmod 600 "$cfg"
    log_ok "prepended Include to ~/.ssh/config (backup in $(pretty_path "$DOTFILES_BACKUP_DIR"))"
    grep -qE '^[[:space:]]*Host[[:space:]]+github\.com' "$DOTFILES_BACKUP_DIR/ssh_config" 2>/dev/null \
      && log_warn "your existing ~/.ssh/config also has a 'Host github.com' block; the managed one now wins and yours is dead config"
  fi
  return 0
}

core_ssh_key() {
  if [[ -f "$CORE_SSH_KEY" ]]; then
    log_skip "ssh key $(pretty_path "$CORE_SSH_KEY")"
    return 0
  fi
  if [[ "${DOTFILES_DRY_RUN:-0}" == "1" ]]; then
    printf '%s  dry generate ssh key %s%s\n' "$_C_DIM" "$(pretty_path "$CORE_SSH_KEY")" "$_C_RESET"
    return 0
  fi
  # ssh-keygen prompts for a passphrase. Generating a passphraseless key
  # unattended is not something to do behind someone's back.
  if [[ "${DOTFILES_ASSUME_YES:-0}" == "1" ]] || is_container || [[ ! -t 0 ]]; then
    log_warn "no ssh key at $(pretty_path "$CORE_SSH_KEY"); generate one with:"
    printf '         ssh-keygen -t ed25519 -C "%s" -f "%s"\n' "$CORE_EMAIL" "$(pretty_path "$CORE_SSH_KEY")"
    return 0
  fi
  log_step "generating an ed25519 ssh key (choose a passphrase when prompted)"
  ssh-keygen -t ed25519 -C "$CORE_EMAIL" -f "$CORE_SSH_KEY" || log_warn "ssh-keygen failed"
  return 0
}

core_ssh_agent() {
  [[ -f "$CORE_SSH_KEY" ]] || return 0
  [[ "${DOTFILES_DRY_RUN:-0}" == "1" ]] && return 0

  local fp; fp="$(ssh-keygen -lf "$CORE_SSH_KEY" 2>/dev/null | awk '{print $2}')"
  if [[ -n "$fp" ]] && ssh-add -l 2>/dev/null | grep -qF "$fp"; then
    log_skip "ssh key already loaded in the agent"
    return 0
  fi
  if is_macos; then
    # --apple-use-keychain stores the passphrase so this survives reboots.
    run "adding key to ssh-agent + keychain" ssh-add --apple-use-keychain "$CORE_SSH_KEY" \
      || log_warn "ssh-add failed (is the agent running?)"
  else
    run "adding key to ssh-agent" ssh-add "$CORE_SSH_KEY" || log_warn "ssh-add failed"
  fi
  return 0
}

core_ssh_github() {
  [[ -f "$CORE_SSH_KEY.pub" ]] || return 0
  has gh || { log_skip "gh not installed; register the key manually at https://github.com/settings/keys"; return 0; }
  [[ "${DOTFILES_DRY_RUN:-0}" == "1" ]] && { printf '%s  dry check github ssh keys%s\n' "$_C_DIM" "$_C_RESET"; return 0; }
  gh auth status >/dev/null 2>&1 || { log_skip "gh not authenticated; run: gh auth login"; return 0; }

  local pub; pub="$(awk '{print $1" "$2}' "$CORE_SSH_KEY.pub")"
  if gh ssh-key list 2>/dev/null | grep -qF "${pub#* }"; then
    log_skip "ssh key already registered on github"
    return 0
  fi

  # Adding a key changes the GitHub account, so it is never done unattended:
  # -y covers local changes, not remote ones.
  if [[ ! -t 0 ]] || [[ "${DOTFILES_ASSUME_YES:-0}" == "1" ]]; then
    log_warn "ssh key is not on your github account. Register it with:"
    printf '         gh ssh-key add %s --title "%s"\n' "$(pretty_path "$CORE_SSH_KEY").pub" "$(hostname -s)"
    return 0
  fi
  if confirm "Add this ssh key to your GitHub account as \"$(hostname -s)\"?"; then
    gh ssh-key add "$CORE_SSH_KEY.pub" --title "$(hostname -s)" \
      && log_ok "registered ssh key on github" \
      || log_warn "gh ssh-key add failed (needs the admin:public_key scope)"
  else
    log_skip "github registration declined"
  fi
  return 0
}

# --- commit signing ----------------------------------------------------------

core_signing_mode() {
  case "$DOTFILES_SIGNING" in
    gpg|ssh|off) printf '%s' "$DOTFILES_SIGNING"; return 0 ;;
  esac
  if has gpg && gpg --list-secret-keys "$CORE_GPG_KEY" >/dev/null 2>&1; then
    printf 'gpg'
  elif [[ -f "$CORE_SSH_KEY.pub" ]]; then
    printf 'ssh'
  else
    printf 'off'
  fi
}

core_gpg_agent() {
  has gpg || return 0
  local agent_conf="$HOME/.gnupg/gpg-agent.conf"
  [[ -f "$agent_conf" ]] && { log_skip "gpg-agent.conf exists"; return 0; }
  [[ "${DOTFILES_DRY_RUN:-0}" != "1" ]] && { mkdir -p "$HOME/.gnupg"; chmod 700 "$HOME/.gnupg"; }
  {
    has pinentry-mac && printf 'pinentry-program %s\n' "$(command -v pinentry-mac)"
    printf 'default-cache-ttl 86400\nmax-cache-ttl 604800\n'
  } | write_file "$agent_conf"
  [[ "${DOTFILES_DRY_RUN:-0}" != "1" ]] && gpgconf --reload gpg-agent 2>/dev/null
  return 0
}

core_configure_signing() {
  local out="$STATE_DIR/gitconfig.signing" mode
  mode="$(core_signing_mode)"
  log_step "commit signing mode: $mode (DOTFILES_SIGNING=$DOTFILES_SIGNING)"

  case "$mode" in
    gpg)
      core_gpg_agent
      write_file "$out" <<SIGNING
# Generated by \`dotfiles install\`: gpg key $CORE_GPG_KEY found in the keyring.
[user]
	signingkey = $CORE_GPG_KEY
[gpg]
	format = openpgp
	program = $(command -v gpg)
[commit]
	gpgsign = true
[tag]
	gpgsign = true
SIGNING
      log_ok "signing with gpg key $CORE_GPG_KEY"
      ;;

    ssh)
      # allowed_signers is what lets `git log --show-signature` verify locally;
      # without it git can sign but not check its own signatures.
      local signers="$HOME/.ssh/allowed_signers"
      local pub; pub="$(awk '{print $1" "$2}' "$CORE_SSH_KEY.pub" 2>/dev/null)"
      if [[ -n "$pub" ]]; then
        if [[ -f "$signers" ]] && grep -qF "${pub#* }" "$signers" 2>/dev/null; then
          log_skip "allowed_signers already lists this key"
        else
          if [[ "${DOTFILES_DRY_RUN:-0}" == "1" ]]; then
            printf '%s  dry append this key to ~/.ssh/allowed_signers%s\n' "$_C_DIM" "$_C_RESET"
          else
            printf '%s namespaces="git" %s\n' "$CORE_EMAIL" "$pub" >> "$signers"
            chmod 600 "$signers"
            log_ok "added key to ~/.ssh/allowed_signers"
          fi
        fi
      fi
      write_file "$out" <<SIGNING
# Generated by \`dotfiles install\`: signing with the ssh key, no gpg needed.
[user]
	signingkey = $CORE_SSH_KEY.pub
[gpg]
	format = ssh
[gpg "ssh"]
	allowedSignersFile = ~/.ssh/allowed_signers
[commit]
	gpgsign = true
[tag]
	gpgsign = true
SIGNING
      log_ok "signing with ssh key $(pretty_path "$CORE_SSH_KEY")"
      has gh && log_warn "GitHub needs this key registered as a SIGNING key too (separate from auth):
         gh auth refresh -h github.com -s admin:ssh_signing_key
         gh ssh-key add $(pretty_path "$CORE_SSH_KEY").pub --type signing --title \"$(hostname -s) signing\"" || true
      ;;

    off)
      [[ -f "$out" ]] && run "removing stale $(pretty_path "$out")" rm -f "$out"
      log_warn "no signing key available — commits will not be signed"
      printf '       Options, cheapest first:\n'
      printf '         dotfiles key import <file.age>   restore your gpg key (see: dotfiles key --help)\n'
      printf '         DOTFILES_SIGNING=ssh dotfiles install ...   sign with the ssh key instead\n\n'
      ;;
  esac
  return 0
}

core_set_login_shell() {
  local zsh_path; zsh_path="$(command -v zsh 2>/dev/null)" || return 0
  [[ "$SHELL" == "$zsh_path" ]] && { log_skip "login shell is already zsh"; return 0; }
  if is_container; then log_skip "container: leaving login shell alone"; return 0; fi
  if ! grep -qx "$zsh_path" /etc/shells 2>/dev/null; then
    log_warn "$zsh_path is not in /etc/shells; add it before chsh"
    return 0
  fi
  if confirm "Change login shell to $zsh_path?"; then
    run "chsh -s $zsh_path" chsh -s "$zsh_path" || log_warn "chsh failed; run it yourself"
  else
    log_skip "login shell unchanged"
  fi
  return 0
}

core_install_packages
core_ssh_config
core_ssh_key
core_ssh_agent
core_ssh_github
core_configure_signing
core_set_login_shell
