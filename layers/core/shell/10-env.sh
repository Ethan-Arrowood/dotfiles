# Portable environment. Sourced by both bash and zsh.

# GPG needs a terminal for prompts. Failed tty calls print "not a tty";
# assign GPG_TTY only on success so that error never becomes its value.
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
