# ccache for native rebuilds (Node.js V8/deps across branch switches).
# gyp bakes the compiler command into the ninja graph, so ./configure has to
# run with these already set or the cache is never consulted.
if command -v ccache >/dev/null 2>&1; then
  export CC="${CC:-ccache cc}"
  export CXX="${CXX:-ccache c++}"
  export CCACHE_DIR="${CCACHE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/ccache}"
fi
