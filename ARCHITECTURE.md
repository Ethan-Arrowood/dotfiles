# How this works

For auditing the code, not using it. [README.md](README.md) covers usage.

## The whole thing in one paragraph

A layer is a name plus a directory of files. [`layer.rs`](src/layer.rs) holds the list of layers; a profile is a text file naming some of them. `install` resolves profile + `--with` − `--without` into an ordered list, runs each layer's install function and two generic steps for every layer, then sets the Dock and writes the shell init.

## The install loop

[`main.rs` `install()`](src/main.rs) is short and worth reading first:

```rust
let mut claimed = skills::Claimed::new();     // skill names, across layers
for l in &selected {
    (l.install)(ctx, l)?;                     // layer-specific: brew, toolchains…
    link::link_home_dir(ctx, &l.path(ctx))?;   // home/*       → $HOME
    skills::install(ctx, l, &mut claimed)?;    // Skillfile    → skills, rules, hooks
}
dock::apply(ctx, &selected)?;                // every Dockfile → the Dock
shellinit::write(ctx, &profile, &selected)?; // → ~/.config/dotfiles/init.sh
```

Everything except `(l.install)` is why a layer usually needs **no code**: it opts in by shipping files. Only layers doing something unusual (installing Homebrew, writing macOS defaults) have an install function with real content.

`Ctx` ([`ctx.rs`](src/ctx.rs)) is passed everywhere and holds: repo root, real `$HOME`, where backups go, and the install-only flags `dry_run`, `assume_yes`, `update` and `signing`. If you want to know whether something can write, look for `ctx.dry_run`. Only `install` sets those flags. Every other command takes the defaults.

Finding the repo ([`find_root`](src/ctx.rs)) looks for a directory containing `layers/`; the git checkout is incidental. In order: `$DOTFILES_ROOT`, then relative to the binary, then the cwd, then the root recorded in the generated `~/.config/dotfiles/init.sh`. `install.sh` exports the variable before handing over, so a first run never needs the fallbacks.

## Everything that touches $HOME

These places link into `$HOME`, all through [`link::link_file`](src/link.rs):

| What | Where | Code |
| --- | --- | --- |
| `home/*` → `$HOME/*` | `.zshrc`, `.gitconfig`, … | [`link.rs`](src/link.rs) |
| `ssh/*.conf` → `~/.ssh/config.d/` | plus one `Include` line prepended to `~/.ssh/config` | [`core.rs`](src/layers/core.rs) |
| `rules/*.md`, `rules/lang/` → `~/.claude/rules/` | from a Skillfile `rules` repo | [`claude.rs`](src/claude.rs) |
| `rules/orgs/<org>.md` → `~/dev/<org>/AGENTS.md` | only where `~/dev/<org>/` exists | [`claude.rs`](src/claude.rs) |
| Skill dirs → `~/.claude/skills/` | symlinks into the cloned repo | [`skills.rs`](src/skills.rs) |
| Ghostty config, `~/.local/bin/code` | mac-apps | [`mac_apps.rs`](src/layers/mac_apps.rs) |
| `~/.local/bin/dispatch` | work-harper | [`work_harper.rs`](src/layers/work_harper.rs) |

**Generated** files, written by [`link::write_file`](src/link.rs) only when their content changes. They live in `~/.config/dotfiles/` and belong to this tool: `init.sh` ([`shellinit.rs`](src/shellinit.rs)), `gitconfig.signing` ([`signing.rs`](src/signing.rs)), `gitconfig.tools` ([`mac_base.rs`](src/layers/mac_base.rs)) and `dock.applied` ([`dock.rs`](src/dock.rs)).

Files changed in place, each backed up or left alone when in doubt:

- `~/.ssh/config` ([`core.rs`](src/layers/core.rs)): the `Include` line is prepended, with the original copied to the backup directory first. A broken symlink there is replaced.
- `~/.ssh/allowed_signers` ([`signing.rs`](src/signing.rs)): this machine's key is appended. A file it can't read is left alone.
- `~/dev/<org>/CLAUDE.md` ([`claude.rs`](src/claude.rs)): moved to the backup directory only when it is exactly the one-line org import older versions wrote, and only when the org's `AGENTS.md` link replaces it.
- `~/.claude/settings.json` and `~/.codex/hooks.json` ([`hooks.rs`](src/hooks.rs)): merged, not replaced. `edit_json` parses the file, adds only what is missing, backs up the original once per run, and writes atomically. A file it can't parse, or a value of the wrong shape, is left untouched with a warning.

Also: Skillfile repos are cloned under `~/dev` (never pulled), `ssh-keygen` creates `~/.ssh/id_ed25519` when you ask it to, and a SessionStart `hook` entry runs its script once ([`hooks.rs`](src/hooks.rs)), with the agent's config directory and `AGENTIC_DEV_DIR` set to match this tool's. agentic's scripts write only under `$HOME` and `~/dev`.

### link_file is the one to read closely

[`link.rs`](src/link.rs) replaces files the user may care about, so it is deliberately boring:

- Destination already links to the right place → skip.
- Destination is a **real directory** → leave it alone and warn. (Your hand-written `~/.claude/skills/learned` survives a Skillfile naming it.)
- Anything else → move to `~/.dotfiles-backup/<timestamp>/<path relative to $HOME>`, then symlink. The relative path matters: keyed on basename alone, `~/.ssh/config` and `~/.config/ghostty/config` collided and `rename` replaces silently.
- An existing backup entry is **refused**, never overwritten.

Seven tests in that file cover exactly these cases, against a throwaway `$HOME`.

## The three dangerous operations

Everything that escapes `$HOME` is behind [`sys::allow_global_changes()`](src/sys.rs):

1. **Package installs**: `brew` ([`brew.rs`](src/brew.rs)) and `sys_install`, which escalates to root. Guarded *inside* `sys_install` so no caller can forget.
2. **macOS system state**: `defaults write` from [`Defaultsfile`](layers/mac-base/Defaultsfile), `scutil --set`, Xcode CLT ([`mac_base.rs`](src/layers/mac_base.rs)), the Dock from each layer's `Dockfile` ([`dock.rs`](src/dock.rs)), and Chrome policies ([`mac_apps.rs`](src/layers/mac_apps.rs)).
3. **Account state**: `chsh` and `ssh-add --apple-use-keychain` ([`core.rs`](src/layers/core.rs)), and key uploads in `dotfiles github` ([`github.rs`](src/github.rs)). `ssh-add` looks `$HOME`-scoped but writes the real login Keychain.

The guard compares `$HOME` against the account's home from `dscl`/`getent`. A mismatch means a test harness, so global changes are skipped. **It fails closed**: if it cannot determine the real home, it blocks rather than allows. `DOTFILES_ALLOW_SANDBOXED_HOME=1` overrides it, and no other value does. CI asserts it fires.

This exists because a sandboxed-`$HOME` test run once installed packages and rewrote system preferences for real.

## Shell config

`~/.zshrc` is a three-line loader. The real file is `~/.config/dotfiles/init.sh`, generated by [`shellinit.rs`](src/shellinit.rs), which enumerates each enabled layer's fragments at *install* time and emits explicit `source` lines:

```sh
. "$DOTFILES_ROOT"/'layers/core/shell/10-env.sh'
if [ -n "${ZSH_VERSION:-}" ]; then
  . "$DOTFILES_ROOT"/'layers/core/zsh/00-options.zsh'
fi
```

Generated rather than globbed at runtime because zsh and bash disagree in ways that bite silently: zsh doesn't word-split unquoted variables, and it aborts on an unmatched glob. The obvious guard for that, `setopt local_options null_glob`, reverts every `setopt` a sourced file performs. Three real bugs lived there. The trade-off: a **new** fragment file needs `dotfiles install` re-run; edits to existing ones are live.

## Signing

[`signing.rs`](src/signing.rs). One method, the machine's own ssh key, and no mode to choose: key present → sign, else don't, with `--no-signing` to opt out. Writes a generated include so `~/.gitconfig` stays static and a machine with no key gets a working git.

No private key material is handled anywhere in this codebase.

## Reading order

1. [`layer.rs`](src/layer.rs): what a layer is. The `registry()` list is the single source of truth for which layers exist
2. [`main.rs` `install()`](src/main.rs): the loop
3. [`link.rs`](src/link.rs): replaces files, with backups
4. [`sys.rs` `allow_global_changes`](src/sys.rs): the guard
5. Anything else, on demand

## What the tests assert

`cargo test`, all against throwaway directories, never the real `$HOME`.

| Area | Asserts |
| --- | --- |
| [`link.rs`](src/link.rs) | backup content, path collisions, real directories untouched, dangling symlinks, refusing to overwrite a backup |
| [`sys.rs`](src/sys.rs) | home resolves, trailing-slash `$HOME` is not a sandbox, only a confirmed real home allows global changes, only the override value `1` counts |
| [`shellinit.rs`](src/shellinit.rs) | zsh fragments are guarded, no runtime globbing, repo paths, fragment names and the profile quoted |
| [`brew.rs`](src/brew.rs) | trailing comments aren't package names, extra words are reported, `PATH` is imported from `brew shellenv` |
| [`skills.rs`](src/skills.rs) | `#` parses as a separator not a comment, discovery finds both repo layouts, `hook` entries name a known agent, `rules` entries parse, extra words and nested repo paths are reported |
| [`hooks.rs`](src/hooks.rs) | adding a hook keeps existing hooks and key order and is idempotent, a `~/` spelling of the same hook counts, Codex's SessionStart hook gets the `startup` matcher |
| [`claude.rs`](src/claude.rs) | org rules link as `AGENTS.md` only where the org directory exists, old one-line imports are moved out, other files stay, a plugin option you set is never overridden |
| [`layer.rs`](src/layer.rs) | unique names, every layer has its directory, `core` can't be dropped, the README example matches the struct |
| [`mac_base.rs`](src/layers/mac_base.rs) | the newest CLT label is picked by version, not listing order; the shipped `Defaultsfile` parses cleanly |
| [`dock.rs`](src/dock.rs) | paths with spaces, flags after the path, paths read back out of the Dock's plist |
| [`signing.rs`](src/signing.rs) | `.pub` is appended, not substituted for an existing extension; the key path is quoted for git config |
| [`github.rs`](src/github.rs) | registered keys are matched by type |
| [`ctx.rs`](src/ctx.rs) | the recorded repo root survives shell quoting |
| [`tmpdir.rs`](src/tmpdir.rs) | the temp dir is 0700, exclusive, and self-cleaning |

CI also runs real installs of the core profile in a bare container ([`ci.yml`](.github/workflows/ci.yml)): packages, ssh config, agentic rules and hook, both shells, a second run changing nothing, the guard, and a signed commit.

Known gaps: the macOS layers have no automated install test, and the GitHub upload path has never executed.
