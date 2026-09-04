#!/usr/bin/env sh
# Bootstrap for a machine with nothing on it.
#
#   curl -fsSL https://raw.githubusercontent.com/Ethan-Arrowood/dotfiles/main/install.sh | sh
#   curl -fsSL .../install.sh | sh -s -- --profile mac-minimal
#
# Clones the repo (HTTPS, since ssh keys will not exist yet) and fetches the
# matching `dotfiles` binary from GitHub Releases, then hands off to it.
# Nothing here needs Rust: the binary is prebuilt.
#
# Deliberately POSIX sh and dependency-free -- this is the one piece that
# cannot assume anything is already installed.
set -eu

REPO="${DOTFILES_REPO:-Ethan-Arrowood/dotfiles}"
REPO_DIR="${DOTFILES_ROOT:-$HOME/dev/dotfiles}"
BIN_DIR="${DOTFILES_BIN_DIR:-$HOME/.local/bin}"
VERSION="${DOTFILES_VERSION:-latest}"

say() { printf '==> %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

if ! command -v git >/dev/null 2>&1; then
  if [ "$(uname -s)" = "Darwin" ]; then
    say "git is missing; triggering the Xcode Command Line Tools installer"
    xcode-select --install 2>/dev/null || true
    die "finish that install, then re-run this script"
  fi
  die "git is required"
fi

# --- repo ---
if [ -d "$REPO_DIR/.git" ]; then
  say "updating $REPO_DIR"
  git -C "$REPO_DIR" pull --ff-only || say "(pull skipped -- local changes?)"
else
  say "cloning into $REPO_DIR"
  mkdir -p "$(dirname "$REPO_DIR")"
  git clone --depth 1 "https://github.com/$REPO.git" "$REPO_DIR"
fi

# --- binary ---
case "$(uname -s)" in
  Darwin) os=apple-darwin ;;
  Linux)  os=unknown-linux-gnu ;;
  *) die "unsupported OS: $(uname -s)" ;;
esac
case "$(uname -m)" in
  arm64|aarch64) arch=aarch64 ;;
  x86_64|amd64)  arch=x86_64 ;;
  *) die "unsupported architecture: $(uname -m)" ;;
esac
target="${arch}-${os}"

if [ "$VERSION" = latest ]; then
  url="https://github.com/$REPO/releases/latest/download/dotfiles-${target}.tar.gz"
else
  url="https://github.com/$REPO/releases/download/$VERSION/dotfiles-${target}.tar.gz"
fi

say "fetching dotfiles-${target}"
mkdir -p "$BIN_DIR"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

if curl -fsSL "$url" -o "$tmp/dotfiles.tar.gz" 2>/dev/null; then
  tar -xzf "$tmp/dotfiles.tar.gz" -C "$tmp"
  mv "$tmp/dotfiles" "$BIN_DIR/dotfiles"
  chmod +x "$BIN_DIR/dotfiles"
  say "installed $BIN_DIR/dotfiles"
else
  # No release yet, or an unbuilt target. Fall back to building from source
  # when a toolchain happens to be present.
  say "no prebuilt binary at $url"
  if command -v cargo >/dev/null 2>&1; then
    say "building from source instead"
    ( cd "$REPO_DIR" && cargo build --release )
    mv "$REPO_DIR/target/release/dotfiles" "$BIN_DIR/dotfiles"
  else
    die "no prebuilt binary and no cargo to build one; install Rust or check the release"
  fi
fi

PATH="$BIN_DIR:$PATH"
export PATH DOTFILES_ROOT="$REPO_DIR"
exec "$BIN_DIR/dotfiles" install "$@"
