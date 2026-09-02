#!/usr/bin/env bash
# Platform detection. Layers declare which platforms they support in
# layer.conf; the runner uses these to refuse mismatched combinations early.

os_kind() {
  case "$(uname -s)" in
    Darwin) echo macos ;;
    Linux)  echo linux ;;
    *)      echo unknown ;;
  esac
}

os_arch() { uname -m; }

is_macos() { [[ "$(os_kind)" == "macos" ]]; }
is_linux() { [[ "$(os_kind)" == "linux" ]]; }

# True inside a container / devcontainer / CI, where interactive prompts and
# GUI app installs make no sense.
is_container() {
  [[ -n "${REMOTE_CONTAINERS:-}${CODESPACES:-}${DEVCONTAINER:-}" ]] && return 0
  [[ -f /.dockerenv ]] && return 0
  [[ -f /run/.containerenv ]] && return 0
  return 1
}

has() { command -v "$1" >/dev/null 2>&1; }

# Root already, or no sudo installed (the usual case inside a container):
# run the command directly rather than failing on a missing sudo.
as_root() {
  if [[ "$(id -u)" -eq 0 ]]; then
    "$@"
  elif has sudo; then
    sudo "$@"
  else
    log_warn "need root for: $* (no sudo available)"
    return 1
  fi
}

# Best-effort system package install, used only by the core layer to get zsh
# and git present. Warns rather than failing when no manager is available.
sys_install() {
  local pkgs=("$@")
  if has apt-get; then
    run "apt-get update" as_root apt-get update -qq
    run "apt-get install ${pkgs[*]}" as_root apt-get install -y --no-install-recommends "${pkgs[@]}"
  elif has dnf; then
    run "dnf install ${pkgs[*]}" as_root dnf install -y "${pkgs[@]}"
  elif has apk; then
    run "apk add ${pkgs[*]}" as_root apk add --no-cache "${pkgs[@]}"
  elif has pacman; then
    run "pacman -S ${pkgs[*]}" as_root pacman -S --noconfirm "${pkgs[@]}"
  elif has brew; then
    run "brew install ${pkgs[*]}" brew install "${pkgs[@]}"
  else
    log_warn "no supported package manager found; install manually: ${pkgs[*]}"
    return 1
  fi
}
