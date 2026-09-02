# Portable environment. Sourced by both bash and zsh.

# gpg needs to know which terminal to prompt on, or signing fails in ways that
# look like git bugs.
GPG_TTY="$(tty 2>/dev/null)" && export GPG_TTY

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
