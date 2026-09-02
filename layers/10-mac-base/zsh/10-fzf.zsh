# fzf keybindings and completion, when installed.
if command -v fzf >/dev/null 2>&1; then
  source <(fzf --zsh) 2>/dev/null || true
  export FZF_DEFAULT_OPTS='--height 40% --layout=reverse --border'
  command -v fd >/dev/null 2>&1 && export FZF_DEFAULT_COMMAND='fd --type f --hidden --exclude .git'
fi
