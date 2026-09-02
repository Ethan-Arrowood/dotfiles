#!/usr/bin/env bash
# Symlinking with backups. Every file a layer places in $HOME goes through
# link_file so re-runs are idempotent and nothing is ever silently destroyed.

DOTFILES_BACKUP_DIR="${DOTFILES_BACKUP_DIR:-$HOME/.dotfiles-backup/$(date +%Y%m%d-%H%M%S)}"

# link_file <source-in-repo> <destination>
link_file() {
  local src="$1" dst="$2"

  [[ -e "$src" ]] || { log_warn "missing source: $src"; return 1; }

  # Already pointing where we want it.
  if [[ -L "$dst" ]] && [[ "$(readlink "$dst")" == "$src" ]]; then
    log_skip "$(pretty_path "$dst") (already linked)"
    return 0
  fi

  if [[ -e "$dst" ]] || [[ -L "$dst" ]]; then
    if [[ "${DOTFILES_DRY_RUN:-0}" == "1" ]]; then
      printf '%s  dry back up %s -> %s%s\n' "$_C_DIM" "$(pretty_path "$dst")" \
        "$(pretty_path "$DOTFILES_BACKUP_DIR")" "$_C_RESET"
    else
      mkdir -p "$DOTFILES_BACKUP_DIR"
      mv "$dst" "$DOTFILES_BACKUP_DIR/$(basename "$dst")"
      log_warn "backed up existing $(pretty_path "$dst") -> $(pretty_path "$DOTFILES_BACKUP_DIR")/"
    fi
  fi

  if [[ "${DOTFILES_DRY_RUN:-0}" == "1" ]]; then
    printf '%s  dry link %s -> %s%s\n' "$_C_DIM" "$(pretty_path "$dst")" "$(pretty_path "$src")" "$_C_RESET"
    return 0
  fi

  mkdir -p "$(dirname "$dst")"
  ln -s "$src" "$dst"
  log_ok "$(pretty_path "$dst") -> $(pretty_path "$src")"
}

# link_home_dir <layer-dir> — links every entry in <layer-dir>/home into $HOME.
link_home_dir() {
  local home_dir="$1/home"
  [[ -d "$home_dir" ]] || return 0
  local entry
  for entry in "$home_dir"/.[!.]* "$home_dir"/*; do
    [[ -e "$entry" ]] || continue
    link_file "$entry" "$HOME/$(basename "$entry")"
  done
}

# write_file <destination> <<<content — generated (non-symlinked) files.
write_file() {
  local dst="$1" content
  content="$(cat)"
  if [[ "${DOTFILES_DRY_RUN:-0}" == "1" ]]; then
    printf '%s  dry write %s (%s lines)%s\n' "$_C_DIM" "$(pretty_path "$dst")" \
      "$(printf '%s\n' "$content" | wc -l | tr -d ' ')" "$_C_RESET"
    return 0
  fi
  mkdir -p "$(dirname "$dst")"
  printf '%s\n' "$content" > "$dst"
  log_ok "wrote $(pretty_path "$dst")"
}

pretty_path() { printf '%s' "${1/#$HOME/~}"; }
