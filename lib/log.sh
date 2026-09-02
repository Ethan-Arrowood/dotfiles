#!/usr/bin/env bash
# Logging helpers. Everything user-facing goes through here so --quiet and
# --dry-run stay consistent across layers.

if [[ -t 1 ]] && [[ -z "${NO_COLOR:-}" ]]; then
  _C_RESET=$'\033[0m'; _C_DIM=$'\033[2m'; _C_BOLD=$'\033[1m'
  _C_RED=$'\033[31m'; _C_GREEN=$'\033[32m'; _C_YELLOW=$'\033[33m'; _C_BLUE=$'\033[34m'
else
  _C_RESET=''; _C_DIM=''; _C_BOLD=''; _C_RED=''; _C_GREEN=''; _C_YELLOW=''; _C_BLUE=''
fi

log_head()  { printf '\n%s==>%s %s%s%s\n' "$_C_BLUE" "$_C_RESET" "$_C_BOLD" "$*" "$_C_RESET"; }
log_step()  { printf '%s  ->%s %s\n' "$_C_BLUE" "$_C_RESET" "$*"; }
log_ok()    { printf '%s   ok%s %s\n' "$_C_GREEN" "$_C_RESET" "$*"; }
log_skip()  { printf '%s  skip %s%s\n' "$_C_DIM" "$*" "$_C_RESET"; }
log_warn()  { printf '%s  warn%s %s\n' "$_C_YELLOW" "$_C_RESET" "$*" >&2; }
log_err()   { printf '%s error%s %s\n' "$_C_RED" "$_C_RESET" "$*" >&2; }
log_die()   { log_err "$*"; exit 1; }

# run <description> <cmd...> — honours DOTFILES_DRY_RUN.
run() {
  local desc="$1"; shift
  if [[ "${DOTFILES_DRY_RUN:-0}" == "1" ]]; then
    printf '%s  dry %s%s\n' "$_C_DIM" "$desc" "$_C_RESET"
    return 0
  fi
  log_step "$desc"
  "$@"
}

confirm() {
  [[ "${DOTFILES_ASSUME_YES:-0}" == "1" ]] && return 0
  [[ "${DOTFILES_DRY_RUN:-0}" == "1" ]] && return 0
  local reply
  printf '%s   ?%s %s [y/N] ' "$_C_YELLOW" "$_C_RESET" "$1"
  read -r reply </dev/tty || return 1
  [[ "$reply" =~ ^[Yy] ]]
}
