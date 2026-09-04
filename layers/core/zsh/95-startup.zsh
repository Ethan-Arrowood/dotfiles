# Land in the dev directory on a fresh terminal, but never inside an editor
# terminal or an agent session, where the cwd was chosen deliberately.
# Set DOTFILES_NO_AUTOCD=1 in ~/.zshrc.local to turn this off.

if [[ -z "$DOTFILES_NO_AUTOCD" ]] \
  && [[ "$TERM_PROGRAM" != "vscode" ]] \
  && [[ -z "$CLAUDECODE$CLAUDE_CODE" ]] \
  && [[ -o interactive ]] \
  && [[ "$PWD" == "$HOME" ]] \
  && [[ -d "$HOME/dev" ]]; then
  cd "$HOME/dev"
fi
