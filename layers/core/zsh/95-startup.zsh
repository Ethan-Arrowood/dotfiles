# Land in the dev directory on a fresh terminal, but never inside an editor
# terminal or an agent session, where the cwd was chosen deliberately.
# Set DOTFILES_NO_AUTOCD=1 in ~/.zshenv.local to turn this off. ~/.zshrc.local
# loads after this runs, so it is too late.

if [[ -z "${DOTFILES_NO_AUTOCD:-}" ]] \
  && [[ "${TERM_PROGRAM:-}" != "vscode" ]] \
  && [[ -z "${CLAUDECODE:-}${CLAUDE_CODE:-}" ]] \
  && [[ -o interactive ]] \
  && [[ "$PWD" == "$HOME" ]] \
  && [[ -d "$HOME/dev" ]]; then
  cd "$HOME/dev"
fi
