# How this works

For auditing the code, not using it — [README.md](README.md) covers usage.

## The whole thing in one paragraph

A layer is a name plus a directory of files. [`layer.rs`](src/layer.rs) holds the list of layers; a profile is a text file naming some of them. `install` resolves profile + `--with` − `--without` into an ordered list, runs each layer's install function, then applies three generic steps to every layer. That's it. Everything else is detail.

## The install loop

[`main.rs` `install()`](src/main.rs) is short and worth reading first:

```rust
for l in &selected {
    (l.install)(ctx, l)?;                   // layer-specific: brew, toolchains…
    link::link_home_dir(ctx, &l.path(ctx))?; // home/*       → $HOME
    skills::install(ctx, l)?;                // Skillfile    → skills, rules, hooks
}
dock::apply(ctx, &selected)?;                // every Dockfile → the Dock
shellinit::write(ctx, &profile, &selected)?; // → ~/.config/dotfiles/init.sh
```

Everything except `(l.install)` is why a layer usually needs **no code** — it opts in by shipping files. Only layers doing something unusual (installing Homebrew, writing macOS defaults) have an install function with real content.

`Ctx` ([`ctx.rs`](src/ctx.rs)) is passed everywhere and holds: repo root, real `$HOME`, where backups go, and the install-only flags `dry_run`, `assume_yes`, `update` and `signing`. If you want to know whether something can write, look for `ctx.dry_run`. Only `install` sets those flags — every other command takes the defaults.

Finding the repo ([`find_root`](src/ctx.rs)) looks for a directory containing `layers/` — the git checkout is incidental. In order: `$DOTFILES_ROOT`, then the root recorded in the generated `~/.config/dotfiles/init.sh`, then relative to the binary, then the cwd. `install.sh` exports the variable before handing over, so a first run never needs the fallbacks.

## Everything that touches $HOME

Four places link into `$HOME`, all through [`link::link_file`](src/link.rs). The files written instead of linked are listed after the table.

| What | Where | Code |
| --- | --- | --- |
| `home/*` → `$HOME/*` | `.zshrc`, `.gitconfig`, … | [`link.rs`](src/link.rs) |
| `ssh/*.conf` → `~/.ssh/config.d/` | plus one `Include` line prepended to `~/.ssh/config` | [`core.rs`](src/layers/core.rs) |
| `rules/*.md`, `rules/lang/` → `~/.claude/rules/` | from a Skillfile `rules` repo | [`claude.rs`](src/claude.rs) |
| Skill dirs → `~/.claude/skills/` | symlinks into the cloned repo | [`skills.rs`](src/skills.rs) |

Plus two **generated** files (written, not linked): `~/.config/dotfiles/init.sh` ([`shellinit.rs`](src/shellinit.rs)) and `~/.config/dotfiles/gitconfig.signing` ([`signing.rs`](src/signing.rs)).

And three files written outside `link_file`:

- `~/dev/<org>/CLAUDE.md` ([`claude.rs`](src/claude.rs)): created with one import line when absent. An existing file is never changed.
- `~/.claude/settings.json` and `~/.codex/hooks.json` ([`hooks.rs`](src/hooks.rs)): merged, not replaced. `edit_json` parses the file, adds only what is missing, backs up the original once per run, and leaves a file it can't parse untouched.

A SessionStart `hook` entry also runs its script once ([`hooks.rs`](src/hooks.rs)), with the agent's config directory set to the one under `$HOME`. agentic's scripts write only under `$HOME` and `~/dev`.

### link_file is the one to read closely

[`link.rs`](src/link.rs) is the only code that can lose data, so it is deliberately boring:

- Destination already links to the right place → skip.
- Destination is a **real directory** → leave it alone and warn. (Your hand-written `~/.claude/skills/learned` survives a Skillfile naming it.)
- Anything else → move to `~/.dotfiles-backup/<timestamp>/<path relative to $HOME>`, then symlink. The relative path matters: keyed on basename alone, `~/.ssh/config` and `~/.config/ghostty/config` collided and `rename` replaces silently.
- An existing backup entry is **refused**, never overwritten.

Seven tests in that file cover exactly these cases, against a throwaway `$HOME`.

## The three dangerous operations

Everything that escapes `$HOME` is behind [`sys::allow_global_changes()`](src/sys.rs):

1. **Package installs** — `brew` ([`brew.rs`](src/brew.rs)) and `sys_install`, which escalates to root. Guarded *inside* `sys_install` so no caller can forget.
2. **macOS system state** — `defaults write` from [`Defaultsfile`](layers/mac-base/Defaultsfile), `scutil --set`, Xcode CLT ([`mac_base.rs`](src/layers/mac_base.rs)), and the Dock from each layer's `Dockfile` ([`dock.rs`](src/dock.rs)).
3. **Account state** — `chsh` and `ssh-add --apple-use-keychain` ([`core.rs`](src/layers/core.rs)). The second looks `$HOME`-scoped but writes the real login Keychain.

The guard compares `$HOME` against the account's home from `dscl`/`getent`. A mismatch means a test harness, so global changes are skipped. **It fails closed** — if it cannot determine the real home it blocks rather than allows. `DOTFILES_ALLOW_SANDBOXED_HOME=1` overrides. CI asserts it fires.

This exists because a sandboxed-`$HOME` test run once installed packages and rewrote system preferences for real.

## Shell config

`~/.zshrc` is a three-line loader. The real file is `~/.config/dotfiles/init.sh`, generated by [`shellinit.rs`](src/shellinit.rs), which enumerates each enabled layer's fragments at *install* time and emits explicit `source` lines:

```sh
. "$DOTFILES_ROOT/layers/core/shell/10-env.sh"
if [ -n "${ZSH_VERSION:-}" ]; then
  . "$DOTFILES_ROOT/layers/core/zsh/00-options.zsh"
fi
```

Generated rather than globbed at runtime because zsh and bash disagree in ways that bite silently: zsh doesn't word-split unquoted variables, and it aborts on an unmatched glob — while the obvious guard for that, `setopt local_options null_glob`, reverts every `setopt` a sourced file performs. Three real bugs lived there. The trade-off: a **new** fragment file needs `dotfiles install` re-run; edits to existing ones are live.

## Signing

[`signing.rs`](src/signing.rs). One method, the machine's own ssh key, and no mode to choose: key present → sign, else don't, with `--no-signing` to opt out. Writes a generated include so `~/.gitconfig` stays static and a machine with no key gets a working git.

No private key material is handled anywhere in this codebase.

## Reading order

1. [`layer.rs`](src/layer.rs) — what a layer is; the `registry()` list is the single source of truth for which layers exist
2. [`main.rs` `install()`](src/main.rs) — the loop
3. [`link.rs`](src/link.rs) — the only code that can lose data
4. [`sys.rs` `allow_global_changes`](src/sys.rs) — the guard
5. Anything else, on demand

## What the tests assert

`cargo test`, all against throwaway directories — never the real `$HOME`.

| Area | Asserts |
| --- | --- |
| [`link.rs`](src/link.rs) | backup content, path collisions, real directories untouched, dangling symlinks, refusing to overwrite a backup |
| [`sys.rs`](src/sys.rs) | home resolves, trailing-slash `$HOME` is not a sandbox, unknown is treated as sandboxed |
| [`shellinit.rs`](src/shellinit.rs) | zsh fragments are guarded, no runtime globbing, shell metacharacters quoted |
| [`brew.rs`](src/brew.rs) | trailing comments aren't package names, `PATH` is imported from `brew shellenv` |
| [`skills.rs`](src/skills.rs) | `#` parses as a separator not a comment, discovery finds both repo layouts, `hook` entries name a known agent, `rules` entries parse |
| [`hooks.rs`](src/hooks.rs) | adding a hook keeps existing hooks and key order and is idempotent, Codex's SessionStart hook gets the `startup` matcher |
| [`claude.rs`](src/claude.rs) | a plugin option you set is never overridden |
| [`layer.rs`](src/layer.rs) | unique names, every layer has its directory, `core` can't be dropped, the README example matches the struct |
| [`mac_base.rs`](src/layers/mac_base.rs) | the newest CLT label is picked by version, not listing order; the shipped `Defaultsfile` parses cleanly |
| [`dock.rs`](src/dock.rs) | paths with spaces, flags after the path, paths read back out of the Dock's plist |
| [`signing.rs`](src/signing.rs) | `.pub` is appended, not substituted for an existing extension |
| [`tmpdir.rs`](src/tmpdir.rs) | the temp dir is 0700, exclusive, and self-cleaning |

CI additionally installs the core profile in a bare container, asserts a second run changes nothing, and asserts an ssh-signed commit verifies.

Known gaps: no test runs a real install (they'd have to touch a real `$HOME`), and the GitHub upload path has never executed.
