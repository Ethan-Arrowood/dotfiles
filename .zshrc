# starship
eval "$(starship init zsh)"
# fnm (Node.js)
eval "$(fnm env --use-on-cd)"

# Rust
export PATH=$PATH:/Users/ethanarrowood/.cargo/bin

# zsh plugins
fpath=(/share/zsh-completions/ $fpath)
source $(brew --prefix)/share/zsh-autosuggestions/zsh-autosuggestions.zsh
source $(brew --prefix)/share/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh
