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
# Deliberately POSIX sh and dependency-free: this is the one piece that
# cannot assume anything is already installed.
set -eu

# Everything lives in main(), invoked on the last line. Piped into a shell, a
# connection that drops mid-transfer would otherwise execute however much
# arrived; this way an incomplete file never reaches the call.
main() {

REPO="${DOTFILES_REPO:-Ethan-Arrowood/dotfiles}"
REPO_DIR="${DOTFILES_ROOT:-$HOME/dev/dotfiles}"
BIN_DIR="${DOTFILES_BIN_DIR:-$HOME/.local/bin}"
VERSION="${DOTFILES_VERSION:-latest}"

say() { printf '==> %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

# A dry run passes through to `dotfiles install -n`. Before that, this script
# must not install anything or move the checkout either.
dry_run=
for a in "$@"; do
  case "$a" in -n|--dry-run) dry_run=1 ;; esac
done

for tool in curl tar; do
  command -v "$tool" >/dev/null 2>&1 || die "$tool is required"
done

# Install the Xcode Command Line Tools headlessly.
#
# `softwareupdate` only offers CLT while this sentinel file exists (an
# undocumented but long-standing Apple behaviour, verified working on macOS
# 26.6). The listing is not version-ordered, so sort rather than taking the
# last line. Mirrors xcode_clt() in src/layers/mac_base.rs, which handles the
# same job for `dotfiles install` run directly.
install_clt() {
  sentinel=/tmp/.com.apple.dt.CommandLineTools.installondemand.in-progress
  touch "$sentinel"
  label=$(softwareupdate -l 2>/dev/null \
    | sed -n 's/^ *\* Label: \(Command Line Tools.*\)$/\1/p' \
    | sort -V | tail -1)
  rm -f "$sentinel"

  [ -n "$label" ] || return 1
  say "installing $label"
  say "    a few minutes, and sudo will ask for your password"
  sudo softwareupdate --install "$label" --verbose || return 1
  git --version >/dev/null 2>&1
}

# `command -v git` is not enough on macOS: /usr/bin/git exists as a stub even
# with no Command Line Tools installed, so the check passes and the clone then
# fails against a GUI prompt. Actually running git is the only real test.
if ! git --version >/dev/null 2>&1; then
  if [ "$(uname -s)" != "Darwin" ]; then
    die "git is required"
  fi
  say "git is not usable yet: the Xcode Command Line Tools are missing"
  [ -z "$dry_run" ] || die "dry run: would install the Command Line Tools; nothing else can run without git"
  if ! install_clt; then
    # No tty for sudo, softwareupdate restricted by MDM, or Apple changed the
    # sentinel. Fall back to the GUI installer and a second run.
    say "could not install them from here; opening Apple's installer instead"
    xcode-select --install 2>/dev/null || true
    say ""
    say "When it finishes, run this same command again and it will carry on."
    exit 1
  fi
  say "Command Line Tools installed"
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
    # The published builds are glibc-linked; on musl (Alpine) they will not
    # run, so build from source there instead of downloading one.
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
  # Could be a missing release, but equally a DNS failure, a proxy, or a rate
  # limit, so show what curl actually said rather than guessing.
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
