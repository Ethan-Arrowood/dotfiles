#!/usr/bin/env bash
# Language toolchains. Each installer is skipped when already present.

langs_node() {
  brew_bin >/dev/null && brew_shellenv
  has fnm || { log_warn "fnm missing; skipping Node install"; return 0; }
  if fnm list 2>/dev/null | grep -q 'v[0-9]'; then
    log_skip "fnm already has Node versions installed"
  else
    run "fnm install --lts"    fnm install --lts    || log_warn "fnm install --lts failed"
    run "fnm install --latest" fnm install --latest || log_warn "fnm install --latest failed"
    run "fnm default lts-latest" fnm default lts-latest || true
  fi
}

langs_rust() {
  if has rustup; then
    log_skip "rustup already installed"
    run "rustup update stable" rustup update stable || log_warn "rustup update failed"
    return 0
  fi
  if [[ "${DOTFILES_DRY_RUN:-0}" == "1" ]]; then
    printf '%s  dry install rustup%s\n' "$_C_DIM" "$_C_RESET"
    return 0
  fi
  log_step "installing rustup"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path \
    || { log_warn "rustup install failed"; return 0; }
  # shellcheck disable=SC1091
  [[ -r "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env"
  run "rustup component add rust-analyzer clippy rustfmt" \
    rustup component add rust-analyzer clippy rustfmt || true
}

langs_python() {
  if has uv; then
    log_skip "uv already installed"
    run "uv self update" uv self update || true
    return 0
  fi
  if [[ "${DOTFILES_DRY_RUN:-0}" == "1" ]]; then
    printf '%s  dry install uv%s\n' "$_C_DIM" "$_C_RESET"
    return 0
  fi
  log_step "installing uv"
  curl -LsSf https://astral.sh/uv/install.sh | sh || { log_warn "uv install failed"; return 0; }
  export PATH="$HOME/.local/bin:$PATH"
  run "uv python install 3.13" uv python install 3.13 || log_warn "uv python install failed"
}

log_step "installing toolchain packages"
brew_bundle "$LAYER_DIR/Brewfile"
langs_node
langs_rust
langs_python
