# Harper local dev servers. Paths assume the standard ~/dev/harperfast layout;
# override HARPER_DEV_ROOT in ~/.zshrc.local if yours differs.
#
# The credentials below are throwaway values for a local-only instance.

HARPER_DEV_ROOT="${HARPER_DEV_ROOT:-$HOME/dev/harperfast}"

alias harper="node $HARPER_DEV_ROOT/harper/dist/bin/harper.js"
alias harper-clear="rm -rf $HOME/harper $HOME/.harperdb"
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
alias hdb-clear="rm -rf $HOME/hdb $HOME/.harperdb"
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
