#!/usr/bin/env sh
# Bootstrap a fresh machine with POSIX sh.
#
#   curl -fsSL https://raw.githubusercontent.com/Ethan-Arrowood/dotfiles/main/install.sh | sh
#   curl -fsSL .../install.sh | sh -s -- --profile mac-minimal
#
# Clone over HTTPS before SSH keys exist, then fetch the release binary.
# Rust is needed only for a source-build fallback.
set -eu

# Call main() last so a truncated piped download cannot execute partial setup.
main() {

REPO="${DOTFILES_REPO:-Ethan-Arrowood/dotfiles}"
REPO_DIR="${DOTFILES_ROOT:-$HOME/dev/dotfiles}"
BIN_DIR="${DOTFILES_BIN_DIR:-$HOME/.local/bin}"
VERSION="${DOTFILES_VERSION:-latest}"

say() { printf '==> %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

# Pass dry-run through to the CLI. Skip CLT installation and checkout updates.
dry_run=
for a in "$@"; do
  case "$a" in -n|--dry-run) dry_run=1 ;; esac
done

for tool in curl tar; do
  command -v "$tool" >/dev/null 2>&1 || die "$tool is required"
done

# macOS ships a git stub without CLT, so command -v is insufficient.
# Run git to check. Use Apple's installer; headless installation is undocumented.
if ! git --version >/dev/null 2>&1; then
  if [ "$(uname -s)" != "Darwin" ]; then
    die "git is required"
  fi
  say "git is not usable yet: the Xcode Command Line Tools are missing"
  [ -z "$dry_run" ] || die "dry run: would open Apple's Command Line Tools installer"
  xcode-select --install 2>/dev/null || true
  say "Install them in the window, then rerun this command."
  exit 1
fi

# --- repo ---
if [ -d "$REPO_DIR/.git" ] && [ -n "$dry_run" ]; then
  say "dry run: leaving $REPO_DIR as it is"
elif [ -d "$REPO_DIR/.git" ]; then
  say "updating $REPO_DIR"
  git -C "$REPO_DIR" pull --ff-only || say "(pull skipped: local changes?)"
else
  say "cloning into $REPO_DIR"
  mkdir -p "$(dirname "$REPO_DIR")"
  git clone --depth 1 "https://github.com/$REPO.git" "$REPO_DIR"
fi

# --- binary ---
case "$(uname -s)" in
  Darwin) os=apple-darwin ;;
  Linux)
    os=unknown-linux-gnu
    # Release binaries need glibc. Build from source on musl (Alpine).
    if [ -f /etc/alpine-release ] || ! ldd --version 2>&1 | grep -qi glibc; then
      say "this looks like a musl system; the prebuilt binary is glibc-linked"
      musl=1
    fi
    ;;
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
trap 'rm -rf "$tmp"' EXIT
trap 'rm -rf "$tmp"; exit 130' INT TERM

if [ -z "${musl:-}" ] && curl -fsSL "$url" -o "$tmp/dotfiles.tar.gz" 2>"$tmp/curl.err"; then
  tar -xzf "$tmp/dotfiles.tar.gz" -C "$tmp"
  mv "$tmp/dotfiles" "$BIN_DIR/dotfiles"
  chmod +x "$BIN_DIR/dotfiles"
  say "installed $BIN_DIR/dotfiles"
else
  # Show curl errors: missing releases, DNS, proxies and rate limits can all fail.
  if [ -z "${musl:-}" ]; then
    say "could not download $url"
    [ -s "$tmp/curl.err" ] && sed 's/^/    /' "$tmp/curl.err"
  fi
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

# exec replaces the process, so the EXIT trap never runs. Clean up first.
rm -rf "$tmp"
trap - EXIT INT TERM

exec "$BIN_DIR/dotfiles" install "$@"
}

main "$@"
