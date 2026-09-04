#!/usr/bin/env bash
# Work layer: shell aliases only.
#
# No git identity override on purpose -- ethan@arrowood.dev is the identity
# everywhere, including work repos. A domain you own outlives every employer,
# and an address you lose control of takes its commit attribution with it.
# If a CLA ever requires a work address, see the commented includeIf example
# in the core .gitconfig.

[[ -d "$HOME/dev/harperfast" ]] || log_warn "~/dev/harperfast does not exist yet; the harper aliases assume that layout"
return 0
