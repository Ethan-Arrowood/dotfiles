# Rust. .cargo/env is also sourced from .zshenv; this covers bash.
[ -r "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
path_prepend "$HOME/.cargo/bin"
