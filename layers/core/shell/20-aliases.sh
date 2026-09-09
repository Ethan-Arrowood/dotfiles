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

# Prefer modern tools when installed, without breaking muscle memory.
command -v bat  >/dev/null 2>&1 && alias cat='bat --paging=never --style=plain'
command -v eza  >/dev/null 2>&1 && { alias ll='eza -l --git'; alias la='eza -la --git'; }
# Deliberately NOT aliasing grep to rg: different regex dialect (rg -E is
# --encoding, so `grep -E 'a|b'` breaks), different recursion default, and it
# honors .gitignore. Use rg directly.
