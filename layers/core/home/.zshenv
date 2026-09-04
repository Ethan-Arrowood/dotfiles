# Sourced by every zsh, including non-interactive ones. Keep it to variables
# that tools need to find, never anything that prints or is slow.

export EDITOR="${EDITOR:-vim}"
export VISUAL="$EDITOR"
export PAGER="${PAGER:-less}"
export LESS="${LESS:-FRX}"

# XDG, so tools stop scattering dotfiles through $HOME.
export XDG_CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
export XDG_CACHE_HOME="${XDG_CACHE_HOME:-$HOME/.cache}"
export XDG_DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
export XDG_STATE_HOME="${XDG_STATE_HOME:-$HOME/.local/state}"

# Rust, when present.
[ -r "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

[ -r "$HOME/.zshenv.local" ] && . "$HOME/.zshenv.local"
