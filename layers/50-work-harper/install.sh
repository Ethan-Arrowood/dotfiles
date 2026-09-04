#!/usr/bin/env bash
# Work layer: Harper dev-server aliases plus the work password manager.
#
# No git identity override on purpose -- ethan@arrowood.dev is the identity
# everywhere, including work repos. A domain you own outlives every employer,
# and an address you lose control of takes its commit attribution with it.
# If a CLA ever requires a work address, see the commented includeIf example
# in the core .gitconfig.

if is_macos; then
  brew_bundle "$LAYER_DIR/Brewfile"
else
  log_skip "not macOS: skipping the LastPass cask"
fi

[[ -d "$HOME/dev/harperfast" ]] || log_warn "~/dev/harperfast does not exist yet; the harper aliases assume that layout"
return 0
