# Skip missing plugins on bare containers.

# -U keeps fpath de-duplicated, so re-sourcing ~/.zshrc does not grow it.
typeset -U fpath

if command -v brew >/dev/null 2>&1; then
  _brew_prefix="$(brew --prefix)"
  [[ -d "$_brew_prefix/share/zsh-completions" ]] && fpath=("$_brew_prefix/share/zsh-completions" $fpath)
  [[ -d "$_brew_prefix/share/zsh/site-functions" ]] && fpath=("$_brew_prefix/share/zsh/site-functions" $fpath)
fi

autoload -Uz compinit
# Cache completions for a day to avoid repeated security scans.
_zcompdump="${XDG_CACHE_HOME:-$HOME/.cache}/zsh/zcompdump"
[[ -d "${_zcompdump:h}" ]] || mkdir -p "${_zcompdump:h}"
if [[ -n "$_zcompdump"(#qN.mh+24) ]]; then
  compinit -d "$_zcompdump"
else
  compinit -C -d "$_zcompdump"
fi

zstyle ':completion:*' menu select
zstyle ':completion:*' matcher-list 'm:{a-zA-Z}={A-Za-z}'   # case-insensitive
zstyle ':completion:*' list-colors "${(s.:.)LS_COLORS:-}"
zstyle ':completion:*' use-cache on
zstyle ':completion:*' cache-path "${XDG_CACHE_HOME:-$HOME/.cache}/zsh/zcompcache"

if [[ -n "${_brew_prefix:-}" ]]; then
  [[ -r "$_brew_prefix/share/zsh-autosuggestions/zsh-autosuggestions.zsh" ]] \
    && source "$_brew_prefix/share/zsh-autosuggestions/zsh-autosuggestions.zsh"
  # Syntax highlighting must be sourced last of the plugins.
  [[ -r "$_brew_prefix/share/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh" ]] \
    && source "$_brew_prefix/share/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh"
fi
unset _brew_prefix _zcompdump

# History search bound to the arrow keys, scoped to what is already typed.
autoload -Uz up-line-or-beginning-search down-line-or-beginning-search
zle -N up-line-or-beginning-search
zle -N down-line-or-beginning-search
bindkey '^[[A' up-line-or-beginning-search
bindkey '^[[B' down-line-or-beginning-search
