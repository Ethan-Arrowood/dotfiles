#!/usr/bin/env bash
# Bootstrap entrypoint for a machine with nothing on it.
#
#   curl -fsSL https://raw.githubusercontent.com/Ethan-Arrowood/dotfiles/main/install.sh | bash
#   curl -fsSL .../install.sh | bash -s -- --profile mac-minimal
#
# Clones the repo (over HTTPS, since ssh keys won't exist yet) and hands off
# to bin/dotfiles. Safe to re-run: an existing clone is updated in place.
set -euo pipefail

REPO_URL="${DOTFILES_REPO_URL:-https://github.com/Ethan-Arrowood/dotfiles.git}"
REPO_DIR="${DOTFILES_ROOT:-$HOME/dev/dotfiles}"

if ! command -v git >/dev/null 2>&1; then
  if [[ "$(uname -s)" == "Darwin" ]]; then
    echo "==> git is missing; triggering the Xcode Command Line Tools installer"
    xcode-select --install 2>/dev/null || true
    echo "    Finish that install, then re-run this script."
    exit 1
  fi
  echo "error: git is required" >&2
  exit 1
fi

if [[ -d "$REPO_DIR/.git" ]]; then
  echo "==> updating $REPO_DIR"
  git -C "$REPO_DIR" pull --ff-only || echo "    (pull skipped — local changes?)"
else
  echo "==> cloning into $REPO_DIR"
  mkdir -p "$(dirname "$REPO_DIR")"
  git clone "$REPO_URL" "$REPO_DIR"
fi

exec "$REPO_DIR/bin/dotfiles" install "$@"
