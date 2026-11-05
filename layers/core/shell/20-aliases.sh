# Portable aliases.

alias ll='ls -lh'
alias la='ls -lAh'
alias ..='cd ..'
alias ...='cd ../..'
alias mkdir='mkdir -p'

# Reload / edit shell config.
alias sz='exec ${SHELL##*/}'
alias ez='${EDITOR:-vim} "$DOTFILES_ROOT"'

# git
alias g='git'
alias gs='git status --short --branch'
alias gd='git diff'
alias gds='git diff --staged'
alias gl='git log --oneline --graph --decorate -20'
alias gco='git checkout'
alias gcm='git commit -m'
alias gca='git commit --amend --no-edit'
alias gp='git push'
alias gpf='git push --force-with-lease'
alias gpl='git pull'
alias gcpc='git cherry-pick --continue'
alias gcps='git cherry-pick --skip'
alias gcpa='git cherry-pick --abort'

# Use optional tools under familiar aliases.
command -v bat  >/dev/null 2>&1 && alias cat='bat --paging=never --style=plain'
command -v eza  >/dev/null 2>&1 && { alias ll='eza -l --git'; alias la='eza -la --git'; }
# Do not alias grep to rg: -E means --encoding in rg, which also recurses
# and honors .gitignore. Use rg directly.
