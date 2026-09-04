#!/usr/bin/env bash
# Essential applications, plus the Claude Code CLI (not a cask).

apps_claude_code() {
  if has claude; then
    log_skip "claude code cli"
    return 0
  fi
  if [[ "${DOTFILES_DRY_RUN:-0}" == "1" ]]; then
    printf '%s  dry install claude code cli%s\n' "$_C_DIM" "$_C_RESET"
    return 0
  fi
  log_step "installing claude code cli"
  curl -fsSL https://claude.ai/install.sh | bash || log_warn "claude code install failed"
}

apps_vscode_cli() {
  # `code` on PATH, without opening VS Code to run the shell-command action.
  local bin="/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code"
  [[ -x "$bin" ]] || return 0
  [[ -L "$HOME/.local/bin/code" ]] && { log_skip "code cli"; return 0; }
  run "linking code cli" bash -c "mkdir -p '$HOME/.local/bin' && ln -sf '$bin' '$HOME/.local/bin/code'"
}

# Chrome policies, written to the user preference domain. An MDM-pushed
# managed preference always wins over these, which is the correct precedence
# on a work machine -- these fill gaps, they do not fight IT.
#
# Verify what actually took effect at chrome://policy.
apps_chrome_defaults() {
  is_macos || return 0
  local -a policies=(
    # Dashlane owns credentials; stop Chrome competing for the same job.
    "PasswordManagerEnabled -bool false"
    "AutofillAddressEnabled -bool false"
    "AutofillCreditCardEnabled -bool false"
    # Quality of life.
    "PromotionalTabsEnabled -bool false"
    "DefaultBrowserSettingEnabled -bool false"
    "MetricsReportingEnabled -bool false"
    "RestoreOnStartup -int 1"
  )
  local p
  for p in "${policies[@]}"; do
    # shellcheck disable=SC2086
    run "defaults write com.google.Chrome $p" defaults write com.google.Chrome $p
  done
  log_warn "Chrome policy changes need a full restart of Chrome; confirm at chrome://policy"
  return 0
}

brew_bundle "$LAYER_DIR/Brewfile"
apps_claude_code
apps_vscode_cli
apps_chrome_defaults
