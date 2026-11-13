# Load layers/*/shell/*.sh and layers/*/zsh/*.zsh through generated init.sh.

# Always ~/.config, never $XDG_CONFIG_HOME: the installer writes it there.
[ -r "$HOME/.config/dotfiles/init.sh" ] && . "$HOME/.config/dotfiles/init.sh"

# Load untracked machine overrides last.
[ -r "$HOME/.zshrc.local" ] && . "$HOME/.zshrc.local"
