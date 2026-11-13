# dotfiles

My machine setup: a Rust CLI that installs layers composed into profiles. [`README.md`](README.md) covers usage. Read [`ARCHITECTURE.md`](ARCHITECTURE.md) before changing code.

## Posting exception

This personal repo has no expected contributors. As an exception to the Posting rules' Human Voice default, you may write commit messages and PR descriptions. Use the exception footer. [Permission](https://github.com/Ethan-Arrowood/agentic/blob/main/rules/posting.md#permission) still applies: never commit, push, or open a PR without my go-ahead.

## Checks

Before committing, run what CI runs:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

CI's `core-on-linux` job installs core in a container. It checks installed files and tools, no `ok` line on rerun, the sandboxed `$HOME` guard, and SSH signature verification. No automated tests cover macOS installs. Check those with `dotfiles install -n`.

## Safety

- Never run `dotfiles install` without `-n` on a real machine unless I ask. Test a real install against scratch directories: set `HOME`, `DOTFILES_SRC_DIR`, and `DOTFILES_ROOT`, and unset `CLAUDE_CONFIG_DIR` and `CODEX_HOME`.
- Every change outside `$HOME` (packages, macOS defaults, the Dock, account state) goes through `sys::allow_global_changes()`.
- Never delete or overwrite a user's file without a backup. Use `link::link_file` to back up and link files, or `link::write_file` for generated files. Keep the file inventory in [ARCHITECTURE.md](ARCHITECTURE.md#everything-that-touches-home) complete.
- This repo is public. No credentials, tokens, internal hostnames, or employer-internal details. Secrets live in Dashlane. Never read, write, or handle private key material.

## Design

- Layers opt in through plain-text files (`Brewfile`, `Skillfile`, `Defaultsfile`, `Dockfile`, `Npmfile`, `home/`). Prefer a new line in a file over new code.
- Write logic in Rust, not shell. [`install.sh`](install.sh) only bootstraps the binary.
- A second run changes nothing. Report malformed config lines with line numbers. Do not skip them silently.
- Merge files that an agent or I also edit (`~/.claude/settings.json`, `~/.codex/hooks.json`). Never replace them.
- Agent skills, rules, and sync hooks belong in [agentic](https://github.com/Ethan-Arrowood/agentic). This repo only installs them.

## Docs

- Update `README.md` and `ARCHITECTURE.md` in the same commit as the code: the layer and Skillfile tables, the list of everything that touches `$HOME`, and the test table.
- Link to code instead of restating it.
- Commit subjects are imperative and sentence case, matching `git log`.
