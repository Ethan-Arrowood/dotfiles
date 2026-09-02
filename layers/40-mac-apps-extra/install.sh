#!/usr/bin/env bash
brew_bundle "$LAYER_DIR/Brewfile"

cat <<'MANUAL'
       These are not on Homebrew — install by hand if you want them:
         App Store: LastPass, Asana, Affinity
         Vendor:    NordLayer (nordlayer.com), FileZilla (filezilla-project.org)
MANUAL
