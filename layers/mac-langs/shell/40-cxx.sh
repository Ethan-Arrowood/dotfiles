# Cache native rebuilds across branches. Set compilers before ./configure:
# gyp embeds them in the ninja graph.
if command -v ccache >/dev/null 2>&1; then
  export CC="${CC:-ccache cc}"
  export CXX="${CXX:-ccache c++}"
  export CCACHE_DIR="${CCACHE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/ccache}"
fi
