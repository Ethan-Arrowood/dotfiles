# zsh behaviour.

HISTFILE="${XDG_STATE_HOME:-$HOME/.local/state}/zsh/history"
[[ -d "${HISTFILE:h}" ]] || mkdir -p "${HISTFILE:h}"
HISTSIZE=100000
SAVEHIST=100000

setopt INC_APPEND_HISTORY      # write as you go, not at shell exit
setopt SHARE_HISTORY           # across concurrent shells
setopt EXTENDED_HISTORY        # record timestamps
setopt HIST_IGNORE_ALL_DUPS
setopt HIST_IGNORE_SPACE       # leading space keeps a command out of history
setopt HIST_REDUCE_BLANKS
setopt HIST_VERIFY             # expand !! for review instead of running it

setopt AUTO_CD                 # `foo/` instead of `cd foo/`
setopt AUTO_PUSHD
setopt PUSHD_IGNORE_DUPS
setopt EXTENDED_GLOB
setopt INTERACTIVE_COMMENTS
setopt NO_BEEP

unsetopt CORRECT CORRECT_ALL   # autocorrect guesses wrong more than it helps
