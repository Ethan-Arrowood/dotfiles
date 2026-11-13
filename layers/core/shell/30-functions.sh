# Portable functions.

# Use the first parent when cherry-picking a merge commit.
gcp() {
  if git rev-parse "$1^2" >/dev/null 2>&1; then
    git cherry-pick -m 1 "$1"
  else
    git cherry-pick "$1"
  fi
}

# Commits on <ref>..HEAD in apply order.
grl() { git rev-list --reverse --first-parent "$1"..HEAD; }

mkcd() { mkdir -p "$1" && cd "$1" || return 1; }

# Fresh worktree for a new branch in Claude Code's layout, .claude/worktrees/.
claude_wt() {
  [ -n "$1" ] || { echo "usage: claude_wt <branch>"; return 1; }
  local root
  root="$(git rev-parse --show-toplevel)" || return 1
  git worktree add "$root/.claude/worktrees/$1" -b "$1" && cd "$root/.claude/worktrees/$1" || return 1
}

openssl_crt() { [ -n "$1" ] || { echo "usage: openssl_crt <cert>"; return 1; }; openssl x509 -in "$1" -text -noout; }
openssl_key() { [ -n "$1" ] || { echo "usage: openssl_key <key>";  return 1; }; openssl rsa  -in "$1" -check -noout; }
openssl_csr() { [ -n "$1" ] || { echo "usage: openssl_csr <csr>";  return 1; }; openssl req  -in "$1" -text -noout -verify; }
