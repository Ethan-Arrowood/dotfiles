#!/usr/bin/env bash
# Homebrew helpers. Casks are treated as best-effort: an app that has been
# renamed or pulled from homebrew-cask warns and moves on rather than
# aborting a machine setup that is otherwise fine.

brew_bin() {
  if [[ -x /opt/homebrew/bin/brew ]]; then echo /opt/homebrew/bin/brew
  elif [[ -x /usr/local/bin/brew ]]; then echo /usr/local/bin/brew
  elif [[ -x /home/linuxbrew/.linuxbrew/bin/brew ]]; then echo /home/linuxbrew/.linuxbrew/bin/brew
  else return 1
  fi
}

brew_shellenv() {
  local b; b="$(brew_bin)" || return 1
  eval "$("$b" shellenv)"
}

ensure_homebrew() {
  if brew_bin >/dev/null; then
    brew_shellenv
    log_skip "homebrew (already installed)"
    return 0
  fi
  if [[ "${DOTFILES_DRY_RUN:-0}" == "1" ]]; then
    printf '%s  dry install homebrew%s\n' "$_C_DIM" "$_C_RESET"
    return 0
  fi
  log_step "installing homebrew"
  NONINTERACTIVE=1 /bin/bash -c \
    "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)" \
    || log_die "homebrew install failed"
  brew_shellenv || log_die "homebrew installed but brew not found on PATH"
}

brew_has_formula() { brew list --formula --versions "$1" >/dev/null 2>&1; }
brew_has_cask()    { brew list --cask --versions "$1" >/dev/null 2>&1; }

brew_install_formula() {
  local f="$1"
  if brew_has_formula "$f"; then log_skip "$f"; return 0; fi
  run "brew install $f" brew install "$f" || log_warn "brew install $f failed"
}

brew_install_cask() {
  local c="$1"
  if brew_has_cask "$c"; then log_skip "$c (cask)"; return 0; fi
  # An app installed by hand or by MDM shouldn't be clobbered.
  local app; app="$(brew info --cask "$c" 2>/dev/null | sed -n 's/^ *\(.*\.app\) (App)$/\1/p' | head -1)"
  if [[ -n "$app" ]] && [[ -d "/Applications/$app" ]]; then
    log_skip "$c (/Applications/$app already present)"
    return 0
  fi
  run "brew install --cask $c" brew install --cask "$c" || log_warn "cask $c failed (renamed or unavailable?)"
}

# brew_bundle <Brewfile> — reads a plain list file: lines of "formula name" or
# "cask name". Comments and blanks ignored. Not `brew bundle` proper, so there
# is no dependency on the bundle tap and per-item failures stay non-fatal.
brew_bundle() {
  local file="$1"
  [[ -f "$file" ]] || { log_warn "no such Brewfile: $file"; return 1; }
  local line kind name
  while IFS= read -r line || [[ -n "$line" ]]; do
    line="${line%%#*}"                       # strip trailing comments
    line="${line#"${line%%[![:space:]]*}"}"  # trim leading space
    line="${line%"${line##*[![:space:]]}"}"  # trim trailing space
    [[ -z "$line" ]] && continue
    kind="${line%% *}"
    name="${line#* }"
    case "$kind" in
      formula) brew_install_formula "$name" ;;
      cask)    brew_install_cask "$name" ;;
      tap)     brew tap "$name" >/dev/null 2>&1 || log_warn "tap $name failed" ;;
      *)       log_warn "unknown entry '$kind $name' in $(basename "$file")" ;;
    esac
  done < "$file"
}
