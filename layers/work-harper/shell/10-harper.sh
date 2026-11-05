# Harper dev servers. Set HARPER_DEV_ROOT in ~/.zshenv.local to override
# ~/dev/harperfast; ~/.zshrc.local loads after alias definitions.
# Credentials below are throwaway values for local instances.

HARPER_DEV_ROOT="${HARPER_DEV_ROOT:-$HOME/dev/harperfast}"

alias harper="node $HARPER_DEV_ROOT/harper/dist/bin/harper.js"
# Expand HOME at call time and reject empty values to avoid deleting /harper.
# Confirm because both clear commands delete ~/.harperdb and differ by one character.
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
