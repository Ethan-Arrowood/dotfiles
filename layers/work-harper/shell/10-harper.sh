# Harper local dev servers. Paths assume the standard ~/dev/harperfast layout;
# set HARPER_DEV_ROOT in ~/.zshenv.local if yours differs (~/.zshrc.local
# loads after these aliases are defined).
#
# The credentials below are throwaway values for a local-only instance.

HARPER_DEV_ROOT="${HARPER_DEV_ROOT:-$HOME/dev/harperfast}"

alias harper="node $HARPER_DEV_ROOT/harper/dist/bin/harper.js"
# A function, not an alias: an alias bakes $HOME in at source time, so an
# empty $HOME would expand to `rm -rf /harper`. Confirms first: both of these
# also delete ~/.harperdb, and they are one character apart.
harper-clear() {
  printf 'remove %s and %s? [y/N] ' "$HOME/harper" "$HOME/.harperdb"
  read -r _reply
  case "$_reply" in [Yy]*) rm -rf "${HOME:?}/harper" "${HOME:?}/.harperdb" ;; esac
}
alias harper-check="curl -sS localhost:9925/health"
alias harper-install="harper install \
  --DEFAULTS_MODE=dev \
  --HDB_ADMIN_USERNAME=admin \
  --HDB_ADMIN_PASSWORD=abc123 \
  --ROOTPATH=$HOME/harper \
  --OPERATIONSAPI_NETWORK_PORT=9925 \
  --NODE_HOSTNAME=localhost \
  --LOGGING_LEVEL=debug \
  --LOGGING_STDSTREAMS=true \
  --THREADS_COUNT=1 \
  --THREADS_DEBUG=false \
  --APPLICATIONS_LOCKDOWN=none \
  --APPLICATIONS_MODULELOADER=none \
  --APPLICATIONS_DEPENDENCYLOADER=native"

alias hdb="node $HARPER_DEV_ROOT/harperdb/bin/harperdb.js"
alias harperdb=hdb
hdb-clear() {
  printf 'remove %s and %s? [y/N] ' "$HOME/hdb" "$HOME/.harperdb"
  read -r _reply
  case "$_reply" in [Yy]*) rm -rf "${HOME:?}/hdb" "${HOME:?}/.harperdb" ;; esac
}
alias hdb-install="hdb install \
  --DEFAULTS_MODE=dev \
  --TC_AGREEMENT=yes \
  --HDB_ADMIN_USERNAME=admin \
  --HDB_ADMIN_PASSWORD=abc123 \
  --ROOTPATH=$HOME/hdb \
  --OPERATIONSAPI_NETWORK_PORT=9925 \
  --REPLICATION_HOSTNAME=localhost \
  --LOGGING_LEVEL=debug \
  --LOGGING_STDSTREAMS=true \
  --THREADS_COUNT=1 \
  --THREADS_DEBUG=false"
