# Start fresh terminals in ~/dev. Preserve editor and agent working directories.
# Set DOTFILES_NO_AUTOCD=1 in ~/.zshenv.local to disable; ~/.zshrc.local loads too late.

if [[ -z "${DOTFILES_NO_AUTOCD:-}" ]] \
  && [[ "${TERM_PROGRAM:-}" != "vscode" ]] \
  && [[ -z "${CLAUDECODE:-}${CLAUDE_CODE:-}" ]] \
  && [[ -o interactive ]] \
  && [[ "$PWD" == "$HOME" ]] \
  && [[ -d "$HOME/dev" ]]; then
  cd "$HOME/dev"
fi
