# How this works

An audit guide. [README.md](README.md) covers usage.

## The install loop

[`layer.rs`](src/layer.rs) defines layers and their install order. Profiles name layers. [`main.rs`](src/main.rs) resolves the profile, `--with` and `--without`. With none of them, it repeats the profile and layers recorded in `~/.config/dotfiles/init.sh`. Then it runs:

```rust
let mut claimed = skills::Claimed::new();
for l in &selected {
    (l.install)(ctx, l)?;
    link::link_home_dir(ctx, &l.path(ctx))?;
    skills::install(ctx, l, &mut claimed)?;
}
dock::apply(ctx, &selected)?;
shellinit::write(ctx, &profile, &selected)?;
```

[`Ctx`](src/ctx.rs) carries the repo root, `$HOME`, backup and state directories, and `dry_run`, `assume_yes`, `update`, `signing` and `verbose`. Only `install` changes those flags, except `verbose`, which every command takes. Other commands use the defaults.

[`find_root`](src/ctx.rs) requires a `layers/` directory. It checks `$DOTFILES_ROOT`, paths relative to the binary, the current directory, then the root recorded in `~/.config/dotfiles/init.sh`. [`install.sh`](install.sh) exports `DOTFILES_ROOT` before invoking the binary.

## Layer assets

Layers opt in by shipping files. `home/` and `Skillfile` processing runs for every layer. Layer install functions handle packages, defaults and other setup.

| Asset | Handling |
| --- | --- |
| `home/*` | Link each entry into `$HOME` |
| `shell/*.sh`, `zsh/*.zsh` | Sort by filename within each layer; generate shell init |
| `ssh/*.conf` | Core links fragments into `~/.ssh/config.d/` |
| `Brewfile` | [`brew.rs`](src/brew.rs): `formula <name>`, `cask <name>`, `tap <name>`, one per line with `#` comments. Taps run first. Report malformed lines with line numbers; warn and continue on failed casks |
| `Skillfile` | [`skills.rs`](src/skills.rs): [syntax](README.md#skills). Find `SKILL.md` at depths 1 and 2; stop descending inside a skill. Track claimed names across layers |
| `Defaultsfile` | [`mac_base.rs`](src/layers/mac_base.rs): `domain key type value`. Report malformed lines with line numbers. Write only changed keys |
| `Shortcuts.json` | [`mac_base.rs`](src/layers/mac_base.rs): `com.apple.symbolichotkeys` entries keyed by shortcut id ([what each does](README.md#macos-shortcuts)). Each entry is merged with `defaults write -dict-add` only when it differs; other ids keep the machine's value. Refresh it from a configured Mac with `defaults export com.apple.symbolichotkeys - \| plutil -extract AppleSymbolicHotKeys json -o - -` |
| `Npmfile` | [`mac_langs.rs`](src/layers/mac_langs.rs): one global npm package per line, `#` comments. Packages missing from `~/.npm-global` install with `npm install --global` under fnm's default Node. Extra words on a line are reported |
| `Dockfile` | [`dock.rs`](src/dock.rs): one path per line, optionally followed by `dockutil` flags. Combine in layer order, skip absent paths, apply after apps install |

The Dock stays unchanged when its paths match and its flags match `~/.config/dotfiles/dock.applied`. The plist's numeric settings do not map directly to `dockutil` flags, so the tool records the last successful specification.

Helpers: [`ui::{head,step,ok,skip,warn,detail}`](src/ui.rs), [`link::{link_file,link_home_dir,write_file}`](src/link.rs), [`brew::bundle`](src/brew.rs), [`sys::{has,which,is_macos,is_container,allow_global_changes}`](src/sys.rs).

## Everything that touches $HOME

Links through [`link::link_file`](src/link.rs):

| Source | Destination | Code |
| --- | --- | --- |
| `home/*` | `$HOME/*`: `.zshrc`, `.zshenv`, `.zprofile`, `.gitconfig`, `.gitignore_global` | [`link.rs`](src/link.rs) |
| `ssh/*.conf` | `~/.ssh/config.d/` | [`core.rs`](src/layers/core.rs) |
| `rules/*.md` and subdirectories except `orgs/` | `~/.claude/rules/` | [`claude.rs`](src/claude.rs) |
| `rules/orgs/<org>.md` | `$DOTFILES_SRC_DIR/<org>/AGENTS.md`, default `~/dev/<org>/AGENTS.md`, where the org directory exists | [`claude.rs`](src/claude.rs) |
| Skill directories | `~/.claude/skills/` | [`skills.rs`](src/skills.rs) |
| Ghostty config, VS Code CLI | `~/.config/ghostty/config`, `~/.local/bin/code` | [`mac_apps.rs`](src/layers/mac_apps.rs) |
| dispatch client | `~/.local/bin/dispatch` | [`work_harper.rs`](src/layers/work_harper.rs) |

Generated state lives in `~/.config/dotfiles/`, regardless of `XDG_CONFIG_HOME`. Git includes use literal paths and cannot expand that variable.

| File | Writer |
| --- | --- |
| `init.sh` | [`shellinit.rs`](src/shellinit.rs), through `link::write_file` |
| `gitconfig.signing` | [`signing.rs`](src/signing.rs), through `link::write_file`; removed when signing is disabled or the public key is absent or invalid |
| `gitconfig.tools` | [`mac_base.rs`](src/layers/mac_base.rs), through `link::write_file`; removed when delta is absent |
| `dock.applied` | [`dock.rs`](src/dock.rs), written directly after a successful Dock update |

`link::write_file` skips unchanged content and replaces symlinks instead of writing through them. It does not back up generated files.

Other changes:

- `~/.ssh/config`: [`core.rs`](src/layers/core.rs) prepends `Include ~/.ssh/config.d/*` after copying the original to the backup directory. It replaces broken symlinks and leaves unreadable files alone.
- `~/.npmrc`: [`mac_langs.rs`](src/layers/mac_langs.rs) merges the keys in [`layers/mac-langs/npmrc`](layers/mac-langs/npmrc), including the shared global prefix `~/.npm-global`, which it creates. Other lines, such as npm's registry auth token, stay. A changed file is backed up first; a link from an older install is replaced with a real file.
- `~/.config/ncurc` ([`ncu.rs`](src/ncu.rs)), only when you run `dotfiles ncu`: moves an existing `~/.ncurc` there and links the old path to it, sets missing keys through `ncu-config` with tokens read at a hidden prompt and passed on stdin (ncu echoes values, so its output is discarded), then sets mode 0600.
- `~/.ssh/allowed_signers`: [`signing.rs`](src/signing.rs) appends a missing key by rewriting the file, without a backup. It leaves unreadable files alone.
- `<src_root>/<org>/CLAUDE.md`: [`claude.rs`](src/claude.rs) backs up an old one-line org import only when an `AGENTS.md` link replaces it. Other content stays.
- `~/.claude/settings.json`, `~/.codex/hooks.json`: [`hooks.rs`](src/hooks.rs) merges missing entries, preserves key order, backs up once per run and writes atomically. Parse errors or wrong value shapes warn without changing the file. Writes preserve permissions and follow existing symlinks to their targets.
- Repos: [`skills.rs`](src/skills.rs) clones Skillfile repos under `$DOTFILES_SRC_DIR` (default `~/dev`). [`work_harper.rs`](src/layers/work_harper.rs) also clones dispatch there. Neither pulls existing checkouts. Both set an HTTPS `origin` that names the same repo to its ssh URL through [`github::use_ssh_origin`](src/github.rs), once an ssh probe to GitHub authenticates. Only `remote.origin.url` changes; `pushurl` stays. When anonymous HTTPS fails and `gh` or ssh isn't set up, [`github::offer_setup`](src/github.rs) offers `gh auth login` and `dotfiles github` once per run. It asks only with a terminal, and `allow_global_changes` guards it.
- This repo's checkout: [`github.rs`](src/github.rs), only when you run `dotfiles github` and ssh authenticates, sets an HTTPS `origin` to ssh. A shallow clone gets `git fetch --unshallow`, and a single-branch one gets `git remote set-branches origin '*'` and a fetch.
- SSH keys: [`core.rs`](src/layers/core.rs) invokes `ssh-keygen` for `$DOTFILES_SSH_KEY` (default `~/.ssh/id_ed25519`) and its `.pub` file. It skips generation in containers or without a terminal for the passphrase. [`signing.rs`](src/signing.rs) reads only the public half.
- Hooks: [`hooks.rs`](src/hooks.rs) runs SessionStart scripts once per install or relink. It creates their destination directories and sets the agent config variable and `AGENTIC_DEV_DIR`. agentic manages `~/.codex/AGENTS.md`, `~/.agents/skills/` and its Claude rule and skill links. See [agentic](https://github.com/Ethan-Arrowood/agentic) for script behavior.
- External installers: [`mac_langs.rs`](src/layers/mac_langs.rs) runs fnm, rustup, uv, and `npm install --global` for Npmfile packages into `~/.npm-global`; [`mac_apps.rs`](src/layers/mac_apps.rs) runs the Claude installer; [`work_harper.rs`](src/layers/work_harper.rs) registers the dispatch marketplace and plugin. These tools manage their own files.
- Shell startup creates zsh [history](layers/core/zsh/00-options.zsh) and [completion caches](layers/core/zsh/10-completion.zsh) under the configured XDG directories.
- Bootstrap: [`install.sh`](install.sh) clones or updates `$DOTFILES_ROOT` (default `$DOTFILES_SRC_DIR/<owner>/<repo>` from `$DOTFILES_REPO`, so `~/dev/ethan-arrowood/dotfiles`) and writes the binary to `$DOTFILES_BIN_DIR` (default `~/.local/bin`).

### link_file is the one to read closely

[`link.rs`](src/link.rs) preserves user files:

- Correct link: skip.
- Real directory: warn and leave it alone, including local skill directories.
- Other file or link: move to `~/.dotfiles-backup/<timestamp>/<path relative to $HOME>`, then symlink.
- Existing backup entry: refuse to overwrite it.

Relative backup paths prevent same-basename collisions such as `~/.ssh/config` and `~/.config/ghostty/config`. `rename` would otherwise replace the first backup silently.

## The three dangerous operations

[`sys::allow_global_changes()`](src/sys.rs) guards CLI operations that can escape `$HOME`:

1. Packages and external installers: [`brew.rs`](src/brew.rs), [`mac_langs.rs`](src/layers/mac_langs.rs), [`mac_apps.rs`](src/layers/mac_apps.rs), and `sys_install`. The latter guards internally because it can escalate to root.
2. macOS state: defaults, shortcuts, computer names and Xcode CLT installation in [`mac_base.rs`](src/layers/mac_base.rs), the [Dock](src/dock.rs), and [Chrome policies](src/layers/mac_apps.rs).
3. Account and agent state: `chsh` and `ssh-add --apple-use-keychain` in [`core.rs`](src/layers/core.rs), [GitHub uploads](src/github.rs), and the [dispatch plugin](src/layers/work_harper.rs). `ssh-add` can write the real login Keychain.

The guard compares canonical `$HOME` and account-home paths from `getent` or `dscl`. A mismatch or failed lookup blocks global changes. Only `DOTFILES_ALLOW_SANDBOXED_HOME=1` overrides it. CI checks that it fires.

A sandboxed test once installed packages and changed real system preferences. This guard prevents that in the CLI; the separate bootstrap script does not call it.

## Shell config

[`shellinit.rs`](src/shellinit.rs) generates explicit source lines for [`~/.zshrc`](layers/core/home/.zshrc):

```sh
. "$DOTFILES_ROOT"/'layers/core/shell/10-env.sh'
if [ -n "${ZSH_VERSION:-}" ]; then
  . "$DOTFILES_ROOT"/'layers/core/zsh/00-options.zsh'
fi
```

Runtime globbing caused three zsh bugs: no word splitting of unquoted variables, errors on unmatched globs, and `setopt local_options null_glob` reverting sourced options. See [fragment edits](README.md#how-it-works).

## Agent config

[`claude.rs`](src/claude.rs) installs global rules separately from `CLAUDE.md`, avoiding merges into a file the user edits. Skillfiles require an explicit `rules` entry, so a skills repo cannot silently opt into global instructions.

Org rules use parent-directory `AGENTS.md` links because user-level `paths:` patterns cannot match GitHub orgs. Existing files stay unless they are dangling links or this tool's old one-line imports.

When org links exist, the tool sets `pluginConfigs."cc-plugin-agents-md@builtin".options.instructionFiles` to `claude-md-and-agents-md`, unless already set. This lets Claude Code load org rules beside a repo's `CLAUDE.md`.

## Signing

[`signing.rs`](src/signing.rs) validates the public key and generates a Git include. [Setup and forwarded-agent signing](README.md#commit-signing) are in the user guide. The Rust code never reads private key material.

## What the tests assert

`cargo test` uses temporary directories for file writes.

| Area | Asserts |
| --- | --- |
| [`link.rs`](src/link.rs) | Idempotence, backup content and collisions, real directories preserved, dangling links, backup overwrite refusal |
| [`sys.rs`](src/sys.rs) | Account home and shell resolve, trailing-slash `$HOME`, unknown home blocks changes, only override `1` counts |
| [`shellinit.rs`](src/shellinit.rs) | zsh guards, no runtime globbing, quoted roots, fragment names and profiles |
| [`brew.rs`](src/brew.rs) | Comments, malformed lines, namespaced formulas, cask apps/receipts/commands, `brew shellenv` imports `PATH` |
| [`skills.rs`](src/skills.rs) | Entry parsing, `#` separators, malformed lines, known hook agents, lowercase owners, discovery depth and exclusions, clone fallbacks stop at the first success, setup offered only after the anonymous attempt fails, failure reasons |
| [`hooks.rs`](src/hooks.rs) | Existing hooks and key order preserved, idempotence, aliases, malformed shapes, Codex `startup` matcher |
| [`claude.rs`](src/claude.rs) | Org links only in existing directories, old imports backed up, user files and plugin choices preserved |
| [`layer.rs`](src/layer.rs) | Unique names, asset directories, mandatory `core`, README struct example |
| [`mac_langs.rs`](src/layers/mac_langs.rs) | `~/.npmrc` merge keeps other lines and is idempotent, shipped Npmfile parses, extra words reported |
| [`ncu.rs`](src/ncu.rs) | missing config detected where ncu reads it (not `~/.ncurc`), only key names read, URL encoding |
| [`mac_base.rs`](src/layers/mac_base.rs) | Shipped Defaultsfile and Shortcuts.json, comments, malformed lines, typed plist values |
| [`dock.rs`](src/dock.rs) | Paths with spaces, flags, plist paths, stray percent signs |
| [`signing.rs`](src/signing.rs) | `.pub` appended, Git config quoting |
| [`main.rs`](src/main.rs) | Recorded layers become `--with`/`--without`, re-run command flags |
| [`github.rs`](src/github.rs) | Key matches include type, setup offered only with gh, a terminal and missing access, ssh URLs only for the same repo in any case, `origin` switch waits for ssh, skips dry runs, keeps `pushurl` and runs once, shallow and single-branch clones get full history and every branch |
| [`ctx.rs`](src/ctx.rs) | Recorded root survives shell quoting |
| [`tmpdir.rs`](src/tmpdir.rs) | 0700 mode, exclusive creation, cleanup |

[CI](.github/workflows/ci.yml) also installs core in a bare container. It checks packages, SSH config, agentic rules and hook, both shells, an unchanged second run, the guard and signed-commit verification.

Known gaps: no automated macOS install test or GitHub upload test. Check macOS layers with `dotfiles install -n`.
