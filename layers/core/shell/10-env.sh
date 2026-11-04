# Portable environment. Sourced by both bash and zsh.

# gpg needs to know which terminal to prompt on, or signing fails in ways that
# look like git bugs.
# `tty` prints "not a tty" and exits non-zero in a non-interactive shell, and
# the && only guards the export, not the assignment, so the variable was left
# holding that string. Only set it when there really is one.
if _df_tty="$(tty 2>/dev/null)" && [ -n "$_df_tty" ]; then
  GPG_TTY="$_df_tty"
  export GPG_TTY
fi
unset _df_tty

# Deduplicating PATH helper, used by later layers.
path_prepend() {
  case ":$PATH:" in
    *":$1:"*) ;;
    *) [ -d "$1" ] && PATH="$1:$PATH" && export PATH ;;
  esac
}
path_append() {
  case ":$PATH:" in
    *":$1:"*) ;;
    *) [ -d "$1" ] && PATH="$PATH:$1" && export PATH ;;
  esac
}

path_prepend "$HOME/.local/bin"
