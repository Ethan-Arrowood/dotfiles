# Use Starship when installed, otherwise a built-in prompt with the Git branch.

if command -v starship >/dev/null 2>&1; then
  eval "$(starship init zsh)"
else
  autoload -Uz vcs_info add-zsh-hook
  zstyle ':vcs_info:git:*' formats ' %F{yellow}%b%f'
  precmd_vcs_info() { vcs_info; }
  # Avoid duplicate precmd hooks and extra Git processes when re-sourcing ~/.zshrc.
  add-zsh-hook precmd precmd_vcs_info
  setopt PROMPT_SUBST
  PROMPT='%F{cyan}%~%f${vcs_info_msg_0_} %(?.%F{green}.%F{red})❯%f '
fi
