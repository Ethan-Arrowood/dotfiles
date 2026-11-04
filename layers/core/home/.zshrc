# Loader only. Real configuration lives in the dotfiles repo, under
# layers/*/shell/*.sh (portable) and layers/*/zsh/*.zsh (zsh-only).
# `dotfiles install` regenerates ~/.config/dotfiles/init.sh to point here.

# Always ~/.config, never $XDG_CONFIG_HOME: the installer writes it there.
[ -r "$HOME/.config/dotfiles/init.sh" ] && . "$HOME/.config/dotfiles/init.sh"

# Machine-local, never tracked. Always last so it can override anything above.
[ -r "$HOME/.zshrc.local" ] && . "$HOME/.zshrc.local"
