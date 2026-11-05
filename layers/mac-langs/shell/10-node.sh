# Node via fnm. --use-on-cd switches versions from .nvmrc / .node-version.
if command -v fnm >/dev/null 2>&1; then
  case "${ZSH_VERSION:+zsh}${BASH_VERSION:+bash}" in
    zsh)  eval "$(fnm env --use-on-cd --shell zsh)" ;;
    bash) eval "$(fnm env --use-on-cd --shell bash)" ;;
  esac
fi

