# dotfiles

Machine setup composed from layers. One repo covers a Linux devcontainer and a fresh MacBook; you pick how much of it applies.

```bash
# fresh machine, nothing installed
curl -fsSL https://raw.githubusercontent.com/Ethan-Arrowood/dotfiles/main/install.sh | sh -s -- --profile mac-dev

# already cloned
dotfiles install --profile mac-dev
```

On a factory-fresh Mac that is genuinely the only command: the script installs the Xcode Command Line Tools itself (sudo will ask for your password once), then clones, downloads the binary and runs it. If that fails — no terminal for sudo, or MDM restrictions — it falls back to Apple's GUI installer and asks you to re-run.

Always safe to run twice: a second run re-links nothing, writes no `defaults` that already match, and does not touch installed toolchains — CI asserts this. Add `--dry-run` to see the plan without touching anything, and `--update` to refresh installed toolchains (rustup, uv), which is otherwise never implicit. The Xcode Command Line Tools are installed if missing and then left alone — they track the OS release, so updating them belongs to Software Update.

Auditing rather than using it? [ARCHITECTURE.md](ARCHITECTURE.md) walks the code: the install loop, every path that touches `$HOME`, the three dangerous operations and their guards, and what the tests actually assert.

## How it is built

A Rust binary (the orchestrator) plus this repo (the layer assets) — the same split [chezmoi](https://chezmoi.io) uses. **Rust is not needed on the target machine**: `install.sh` downloads a prebuilt binary for the platform from GitHub Releases. It falls back to `cargo build` only if no release matches.

The assets stay as plain text on purpose — Brewfiles, `.zshrc` fragments, ssh config, the Ghostty config. Those are configuration, not code, and they have to be symlinkable so an edit in `$HOME` and an edit in the repo are the same edit. Embedding them in the binary would break that.

```
src/            the orchestrator
  layer.rs      registry: the single source of truth for which layers exist
  layers/       one module per layer
  shellinit.rs  generates ~/.config/dotfiles/init.sh
  signing.rs    ssh commit signing
layers/         layer assets (symlinked into $HOME)
profiles/       named layer sets
```

### Why not shell

It was shell first. Five bugs in one sitting made the case for a rewrite, and each is now a test:

| Bug | Cause | Test |
| --- | --- | --- |
| Layer loop broke at 2+ layers | zsh does not word-split unquoted vars | `no_runtime_globbing_or_word_splitting` |
| Every `setopt` silently reverted | `local_options` restores on return | `zsh_fragments_are_guarded_...` |
| Install truncated, no output | `has gh &&` returning 1 under `set -e` | `Result` makes it unrepresentable |
| Install aborted on a bad config | unguarded `$(...)` under `set -e` | same |
| Brewfile comments became package names | naive `read` | `trailing_comments_are_not_part_of_the_name` |

Run them with `cargo test`. CI additionally installs the core profile in a bare `debian:bookworm-slim` container, checks a second run is idempotent, and asserts an ssh-signed commit verifies (`%G? = G`).

## Profiles

| Profile | Layers | For |
| --- | --- | --- |
| `core` | core | Linux devcontainers, CI images, remote boxes |
| `mac-minimal` | + mac-base, mac-apps | VMs and loaners: browse, edit, run an agent |
| `mac-dev` | + mac-langs | Working dev machine, no quality-of-life apps |
| `mac-full` | + mac-apps-extra | Daily driver |

Profiles are starting points, not fixed sets:

```bash
dotfiles install --profile mac-full --without mac-apps-extra   # skip Spotify/Zoom/etc
dotfiles install --profile mac-full --with work-harper         # add work config
dotfiles install --profile core --with mac-apps                # build your own
```

## Layers

| Layer | Platform | Contents |
| --- | --- | --- |
| `core` | any | zsh, git, ssh, commit signing, aliases, functions |
| `mac-base` | macOS | Xcode CLT, Homebrew, CLI tools, macOS defaults, Dock, computer name |
| `mac-langs` | macOS | Node (fnm), Rust (rustup), Python (uv), C/C++, Java |
| `mac-apps` | macOS | Chrome, VS Code, Claude, Ghostty, Dashlane CLI |
| `mac-apps-extra` | macOS | Slack, Zoom, Spotify, Discord, Podman, DaisyDisk |
| `work-harper` | any | Codex, Antigravity (`agy`) and gcloud CLIs (macOS), the [dispatch](src/layers/work_harper.rs) client and Claude plugin, Harper dev-server aliases, LastPass (opt-in; no identity override) |

`core` is the only layer that has to work everywhere, so it assumes nothing: no Homebrew, no Node, no GUI. Every optional integration in it — starship, fzf, the zsh plugins, git-delta — is guarded by a `command -v` check and degrades to a sane built-in.

## Commands

```bash
dotfiles install [--profile NAME] [--with LAYER] [--without LAYER] [--dry-run] [--yes]
                 [--no-signing] [--update]
dotfiles list      # profiles and layers
dotfiles status    # what this machine has installed, and where links point
dotfiles link      # re-link $HOME files only, no packages
dotfiles doctor    # missing tools, broken links, key expiry
dotfiles github    # register this machine's ssh key with GitHub (auth + signing)
```

### Safety

Anything machine-global — `brew install`, `defaults write`, `chsh`, `ssh-add --apple-use-keychain`, the toolchain installers — is skipped when `$HOME` is not your real home directory, because that means a test harness with a sandboxed `$HOME`. File operations follow `$HOME` and stay contained; these do not, and would escape the sandbox.

The guard **fails closed**: if it cannot determine your real home, it blocks rather than allows. `DOTFILES_ALLOW_SANDBOXED_HOME=1` is the escape hatch, and CI asserts the guard actually fires.

## How it works

```
layers/core/
├── home/          symlinked into $HOME
├── shell/*.sh     POSIX fragments — sourced by bash and zsh
├── zsh/*.zsh      zsh-only fragments
└── ssh/*.conf     ~/.ssh/config.d/ fragments
```

The directory name **is** the layer name, and install order is the declaration order of `registry()` in `src/layer.rs` — one ordered list, one source of truth. (The shell version numbered these directories because a glob decided the order; nothing globs any more, so the numbers were just a second place for the ordering to drift.)

Numbers on the *fragment files* are still load-bearing: they order files **within** a layer, and that comes from a filesystem sort.

`~/.zshrc` is a three-line loader symlinked out of this repo. The real work happens in `~/.config/dotfiles/init.sh`, generated at install time, which pins `DOTFILES_ROOT` and lists each fragment as an explicit `source` line:

```sh
# --- core ---
. "$DOTFILES_ROOT/layers/core/shell/10-env.sh"
if [ -n "${ZSH_VERSION:-}" ]; then
  . "$DOTFILES_ROOT/layers/core/zsh/00-options.zsh"
fi
```

**Editing** a fragment takes effect in the next shell. **Adding** one means re-running `dotfiles install` so it gets listed — cheap, since every step is a no-op when already done.

Files in `$HOME` are symlinks back into the repo, so `vim ~/.zshrc` and `git diff` here show the same edit. Anything already present is moved to `~/.dotfiles-backup/<timestamp>/` first.

One consequence to watch: an installer that appends to `~/.zshrc` or `~/.zshenv` follows the symlink and writes **into this repo**. rustup and uv are both invoked with the flags that stop them doing so (`--no-modify-path`, `INSTALLER_NO_MODIFY_PATH=1`), but a tool you run by hand can still do it. It shows up as an unexpected `git diff` here rather than as invisible local drift — review it rather than committing it blind.

## Adding a layer

Two steps. Add a module under `src/layers/`:

```rust
// src/layers/my_thing.rs
pub fn install(ctx: &Ctx, layer: &Layer) -> Result<()> {
    brew::bundle(ctx, &layer.path(ctx).join("Brewfile"))?;
    Ok(())
}
```

and an entry in the `registry()` in `src/layer.rs`:

```rust
Layer {
    name: "my-thing",
    description: "What it does.",
    macos_only: false,
    install: layers::my_thing::install,
},
```

Position in that list is the install order. Then create `layers/my-thing/` for its assets — a test fails if the directory is missing, since the name is the only thing pointing at it. It appears in `dotfiles list` immediately.

Helpers available: `ui::{head,step,ok,skip,warn,detail}`, `link::{link_file, link_home_dir,write_file}`, `brew::bundle`, `sys::{has,which,is_macos, is_container,allow_global_changes}`.

`Brewfile`s are a plain list, not `brew bundle` syntax — `formula <name>`, `cask <name>`, `tap <name>`, one per line, `#` comments allowed. A cask that has been renamed or pulled warns and the run continues.

macOS settings live in [`layers/mac-base/Defaultsfile`](layers/mac-base/Defaultsfile), one `domain key type value` per line. Each key is read first and only written when it differs.

A `Dockfile` lists Dock items, one path per line, optionally followed by `dockutil` flags. Every selected layer's `Dockfile` is combined in layer order and the Dock is set to exactly that, after all layers have installed, so Chrome (from [`mac-apps`](layers/mac-apps/Dockfile)) joins mac-base's [items](layers/mac-base/Dockfile) only on a machine that has it.

## Claude Code config

Global instructions are **rule files**, not a `CLAUDE.md`. Everything in `~/.claude/rules/*.md` loads every session, so a repo can own whole topic files while `~/.claude/CLAUDE.md` stays untracked and machine-specific:

```
~/.claude/rules/general.md   -> symlink into Ethan-Arrowood/skills  (tracked, shared)
~/.claude/CLAUDE.md          -> machine-local, untracked
```

They live in the [skills repo](https://github.com/Ethan-Arrowood/skills/tree/main/rules), beside the skills, and a `rules <owner>/<repo>` line in a [Skillfile](layers/core/Skillfile) links that repo's `rules/*.md` into `~/.claude/rules/`. It is a separate line from `repo` so an org skills repo that grows a `rules/` directory cannot start loading into every session unasked. Nothing merges markdown sections, so a hand edit to `CLAUDE.md` is never clobbered by a reinstall.

New or changed rule files load at the **start of the next session** — a rules directory that did not exist when a session started is not watched.

Note: Claude Code skips a symlinked user-scope rule file in Cowork sessions on the desktop. Everywhere else it follows the link normally.

## Skills

Skills are not vendored here. Their source lives in its own repository, which gets cloned once and then symlinked into `~/.claude/skills/`. Claude Code follows a symlinked skill directory and reads `SKILL.md` from the target, so the clone is simultaneously the installed skill *and* an editable checkout — edit it in place and the change is live, with no re-sync step.

A layer declares what it wants in a `Skillfile`, same format as a `Brewfile`:

```
repo  <owner>/<repo>            every skill in the repo
skill <owner>/<repo>#<skill>    just that one
rules <owner>/<repo>            its rules/*.md -> ~/.claude/rules/
```

```
# layers/core/Skillfile
repo Ethan-Arrowood/skills
rules Ethan-Arrowood/skills

# layers/work-harper/Skillfile
repo harperfast/skills
repo harperfast/skills-internal
```

Repos clone to `$DOTFILES_SRC_DIR/<owner>/<repo>`, default `~/dev`, with the owner lowercased so `Ethan-Arrowood/skills` lands beside `~/dev/ethan-arrowood/`. `gh repo clone` is used when available so private and internal repos work; otherwise plain `git clone` over ssh.

An existing checkout is **never pulled or modified** — these are working trees you edit, so updating them is your call, not the installer's.

Skills are found by locating `SKILL.md` rather than by assuming a path, because layouts differ: `Ethan-Arrowood/skills` and `harperfast/skills` put skills at the top level, while `harperfast/skills-internal` nests them under `skills/`. The search is bounded to two levels and stops descending once a directory is identified as a skill, so an example `SKILL.md` inside a skill does not become a phantom entry.

A local-only skill that is a real directory in `~/.claude/skills/` coexists fine — nothing touches a name no `Skillfile` declares.

## Machine-local escape hatches

Nothing here needs a fork:

| File | Purpose |
| --- | --- |
| `~/.zshrc.local` | Sourced last by every interactive zsh |
| `~/.zshenv.local` | Sourced by every zsh, including non-interactive |
| `~/.zprofile.local` | Login shells |
| `~/.gitconfig.local` | Git settings for this machine only |

## SSH

The core layer sets SSH up on every machine, macOS or container:

- `~/.ssh/config.d/` fragments, symlinked from the repo. `~/.ssh/config` gets a single `Include ~/.ssh/config.d/*` line prepended (original backed up), so your own host entries stay where they are.
- `Host github.com` with `User git`, `AddKeysToAgent`, `UseKeychain` and the identity file. `IgnoreUnknown UseKeychain` keeps the same file working on Linux, where that option does not exist and would otherwise make ssh refuse to parse the config at all.
- Generates an ed25519 key if there isn't one (it prompts for a passphrase; unattended runs print the command instead of making a passphraseless key).
- Loads it into the agent, with `--apple-use-keychain` on macOS so it survives reboots.

`Host *` defaults live in `99-defaults.conf`, numbered last on purpose: ssh takes the **first** value it sees for a keyword, so a `Host *` block placed early silently overrides every specific host after it.

Git rewrites `https://github.com/` to `git@github.com:`, so every clone uses SSH regardless of the URL you paste.

## Registering keys with GitHub

```bash
dotfiles github
```

GitHub treats these as two separate uploads of the same key, which is the part worth not having to remember:

| What | Purpose | Token scope |
| --- | --- | --- |
| ssh **authentication** key | `git push` works | `admin:public_key` |
| ssh **signing** key | commits show Verified | `admin:ssh_signing_key` |

The same ssh key has to be uploaded **twice**, once as each type — that is the step everyone forgets. `dotfiles github` checks what is already registered, requests any missing token scope for you, uploads only what is needed, and then verifies by authenticating over ssh and making a real signed commit in a temp repo. Every upload asks first; nothing touches your account unattended.

## Commit signing

One method: **the machine's own ssh key**. The same key that pushes also signs.

There is no mode to choose — a machine either has a key or it does not. If it does, commits are signed; if not, `install` says so and tells you how to make one. `--no-signing` opts out, and exists for CI.

Nothing is written into `~/.gitconfig`. The settings go in a generated include ([`signing.rs`](src/signing.rs)), so a machine with no key gets a working git rather than one that fails on every commit. Signing also writes `~/.ssh/allowed_signers`, without which git can create signatures but not verify its own.

**The catch:** GitHub keeps *two* lists — authentication keys and signing keys — and the same public key has to be uploaded to both. Pushing works with only the first, so a missing signing key shows up as **Unverified** commits and nothing else. `dotfiles github` does both uploads; `dotfiles install` checks and tells you if one is missing.

## Machines, containers and remote boxes

The rule: **the private key lives on laptops only.** Everything else borrows it through the agent, or gets its own.

| Where | Auth (push) | Signing | What to do |
| --- | --- | --- | --- |
| Your laptops | own key each | own key each | `dotfiles install` |
| Devcontainer | host agent, forwarded | host agent, forwarded | `dotfiles install --profile core` inside |
| Remote box you use often | own key | own key | `dotfiles install --profile core` there |
| Remote box, one-off | forwarded agent | forwarded agent | `ssh -A`, scoped to that host |
| CI | deploy key / token | usually unsigned | out of scope for this repo |

### Why the agent is enough

`ssh-keygen -Y sign` will use a key held by the agent when the private key file is not on disk. So a forwarded agent gives a container or remote box the ability to both push *and* sign, while the private key never leaves your laptop. Verified: with the private key file deleted and only the agent holding it, `git commit` produces `%G? = G`.

This is why signing is ssh-only. Agent forwarding is built into ssh and into VS Code's devcontainer and Remote-SSH support; the gpg equivalent means forwarding the gpg-agent socket via `RemoteForward`, with matching versions and `extra-socket` on both ends. It works, but it is a thing you maintain forever.

### Devcontainers, specifically

VS Code already forwards your ssh agent and copies your `~/.gitconfig` into the container. That gets you push and the right author identity — but **not signing**, because the copied gitconfig's `[include]` lines point at `~/.config/dotfiles/gitconfig.signing`, which does not exist in the container. Git ignores missing includes silently, so commits come out unsigned with no warning.

Fix it by running the core layer inside the container:

```jsonc
// .devcontainer/devcontainer.json
{
  "postCreateCommand": "curl -fsSL https://raw.githubusercontent.com/Ethan-Arrowood/dotfiles/main/install.sh | sh -s -- --profile core -y",
  "mounts": ["source=${localEnv:HOME}/.ssh/id_ed25519.pub,target=/tmp/id_ed25519.pub,type=bind,readonly"],
  "remoteEnv": { "DOTFILES_SSH_KEY": "/tmp/id_ed25519" }
}
```

`DOTFILES_SSH_KEY` is the load-bearing line. Without it the installer looks for `~/.ssh/id_ed25519.pub`, which does not exist in the container, and falls through to unsigned — the exact silent failure described above. It points at the *private* key path; the `.pub` suffix is appended, and only the public half needs to be mounted.

The core layer detects the container, leaves the login shell alone, skips key generation, and configures signing against the forwarded agent. Only the *public* key is mounted — the private key stays on the host.

### Remote servers

For a box you use regularly, give it its own key and run `dotfiles install --profile core` on it. Per-machine keys mean revoking one machine is one click in GitHub, with nothing else to rotate.

For a one-off, forward the agent — but scope it to that host, never globally:

```
# ~/.ssh/config.d/50-mybox.conf
Host mybox
    ForwardAgent yes
```

`ForwardAgent yes` under `Host *` would hand your agent to every server you touch, and anyone with root on the far end can use it for as long as you are connected. That is why this repo's `99-defaults.conf` does not set it.

## The old GPG key (historical)

Commits were GPG-signed through 2026 with key `79007EFD4678CB94` (RSA 4096, expires 2028-07-19). **Dashlane holds both halves** — that is the only copy, and it is where it should stay.

This tool no longer touches it. Signing is ssh-only, so nothing here needs a portable private key, and the code never handles key material. If you ever need the old key back — verifying an old signature, say:

```bash
dcli read 'dl://<secret-id>' | gpg --import
gpg --edit-key 79007EFD4678CB94 trust   # 5 (ultimate), then quit
```

Find the id with `dcli password "GPG signing key" -o json`. That is a manual step on purpose: a key you need once every few years is not worth a code path.
