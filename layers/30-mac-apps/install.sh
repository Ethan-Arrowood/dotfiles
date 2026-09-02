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

brew_bundle "$LAYER_DIR/Brewfile"
apps_claude_code
apps_vscode_cli
