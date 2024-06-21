# Install Homebrew and Command Line Tools for XCode
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# Initialize Brew
eval "$(/opt/homebrew/bin/brew shellenv)"

# Install Applications
brew install --cask \
	font-fira-code \
	font-fira-code-nerd-font \
	google-chrome \
	rectangle \
	slack \
	visual-studio-code

# Remind about non-automatable apps
echo "Install Copilot, Hotspot Shield, and Dashlane from App Store"
echo "Install Sizzy from https://sizzy.co and Spotify from https://spotify.com"

# Install CLI Tools
brew install \
	fnm \
	pnpm \
	wget \
	zsh-completions \
	starship

# Copy profile and rc files
cp ./.zshrc ~/.zshrc
cp ./.zprofile ~/.zprofile

# Install latest Node.js versions
fnm install --lts
fnm install --latest

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh