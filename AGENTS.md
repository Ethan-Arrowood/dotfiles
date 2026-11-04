# dotfiles

My personal machine setup: a Rust CLI that installs layers composed into profiles. [`README.md`](README.md) covers usage. [`ARCHITECTURE.md`](ARCHITECTURE.md) covers how it works; read it before changing code.

## Posting exception

This is an exception to the Human Voice default in the Posting rules. It's my personal repo and I don't expect other contributors, so you may write commit messages and PR descriptions here yourself. Use the exception footer. [Permission](https://github.com/Ethan-Arrowood/agentic/blob/main/rules/posting.md#permission) still applies: never commit, push, or open a PR without my go-ahead.

## Checks

Before committing, run what CI runs:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

CI's `core-on-linux` job also runs real installs of the core profile in a container. It checks what core installs, that a second run prints no `ok` line, that a sandboxed `$HOME` blocks machine-global changes, and that an ssh-signed commit verifies. Nothing tests the macOS layers automatically, so check those with `dotfiles install -n`.

## Safety

- Never run `dotfiles install` without `-n` on a real machine unless I ask. Test a real install against scratch directories: set `HOME`, `DOTFILES_SRC_DIR`, and `DOTFILES_ROOT`, and unset `CLAUDE_CONFIG_DIR` and `CODEX_HOME`.
- Every change outside `$HOME` (packages, macOS defaults, the Dock, account state) goes through `sys::allow_global_changes()`.
- Never delete or overwrite a user's file without a backup. Use `link::link_file`, which moves the old file to the backup directory first, or `link::write_file` for files this tool generates. ARCHITECTURE.md lists every file the tool changes; keep that list complete.
- This repo is public. No credentials, tokens, internal hostnames, or employer-internal details. Secrets live in Dashlane. Never read, write, or handle private key material.

## Design

- A layer opts in by shipping a plain-text file (`Brewfile`, `Skillfile`, `Defaultsfile`, `Dockfile`, `home/`). Prefer a new line in a file over new code.
- Write logic in Rust, not shell. [`install.sh`](install.sh) only bootstraps the binary.
- A second run changes nothing. Report malformed config lines with their line number instead of skipping them.
- Merge files that an agent or I also edit (`~/.claude/settings.json`, `~/.codex/hooks.json`). Never replace them.
- Agent skills, rules, and sync hooks belong in [agentic](https://github.com/Ethan-Arrowood/agentic). This repo only installs them.

## Docs

- Update `README.md` and `ARCHITECTURE.md` in the same commit as the code: the layer and Skillfile tables, the list of everything that touches `$HOME`, and the test table.
- Link to code instead of restating it.
- Commit subjects are imperative and sentence case, matching `git log`.
