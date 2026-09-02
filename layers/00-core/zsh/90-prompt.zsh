# Prompt. Starship when available, otherwise a small built-in with git branch
# so a bare container still shows something useful.

if command -v starship >/dev/null 2>&1; then
  eval "$(starship init zsh)"
else
  autoload -Uz vcs_info
  zstyle ':vcs_info:git:*' formats ' %F{yellow}%b%f'
  precmd_vcs_info() { vcs_info; }
  precmd_functions+=(precmd_vcs_info)
  setopt PROMPT_SUBST
  PROMPT='%F{cyan}%~%f${vcs_info_msg_0_} %(?.%F{green}.%F{red})❯%f '
fi
