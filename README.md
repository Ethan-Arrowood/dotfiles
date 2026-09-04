# dotfiles

Machine setup composed from layers. One repo covers a Linux devcontainer and a
fresh MacBook; you pick how much of it applies.

```bash
# fresh machine, nothing installed
curl -fsSL https://raw.githubusercontent.com/Ethan-Arrowood/dotfiles/main/install.sh | sh -s -- --profile mac-dev

# already cloned
dotfiles install --profile mac-dev
```

Always safe to run twice. Add `--dry-run` to see the plan without touching
anything.

## How it is built

A Rust binary (the orchestrator) plus this repo (the layer assets) — the same
split [chezmoi](https://chezmoi.io) uses. **Rust is not needed on the target
machine**: `install.sh` downloads a prebuilt binary for the platform from
GitHub Releases. It falls back to `cargo build` only if no release matches.

The assets stay as plain text on purpose — Brewfiles, `.zshrc` fragments, ssh
config, the Ghostty config. Those are configuration, not code, and they have to
be symlinkable so an edit in `$HOME` and an edit in the repo are the same edit.
Embedding them in the binary would break that.

```
src/            the orchestrator
  layer.rs      registry: the single source of truth for which layers exist
  layers/       one module per layer
  shellinit.rs  generates ~/.config/dotfiles/init.sh
  signing.rs    ssh vs gpg commit signing
layers/         layer assets (symlinked into $HOME)
profiles/       named layer sets
```

### Why not shell

It was shell first. Five bugs in one sitting made the case for a rewrite, and
each is now a test:

| Bug | Cause | Test |
| --- | --- | --- |
| Layer loop broke at 2+ layers | zsh does not word-split unquoted vars | `no_runtime_globbing_or_word_splitting` |
| Every `setopt` silently reverted | `local_options` restores on return | `zsh_fragments_are_guarded_...` |
| Install truncated, no output | `has gh &&` returning 1 under `set -e` | `Result` makes it unrepresentable |
| Install aborted on a bad config | unguarded `$(...)` under `set -e` | same |
| Brewfile comments became package names | naive `read` | `trailing_comments_are_not_part_of_the_name` |

Run them with `cargo test`. CI additionally installs the core profile in a bare
`debian:bookworm-slim` container, checks a second run is idempotent, and asserts
an ssh-signed commit verifies (`%G? = G`).

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
| `core` | any | zsh, git, commit signing, aliases, functions |
| `mac-base` | macOS | Xcode CLT, Homebrew, CLI tools, macOS defaults, computer name |
| `mac-langs` | macOS | Node (fnm), Rust (rustup), Python (uv), C/C++, Java |
| `mac-apps` | macOS | Chrome, VS Code, Claude, Ghostty, Rectangle |
| `mac-apps-extra` | macOS | Slack, Zoom, Spotify, Discord, Podman, Postman |
| `work-harper` | any | Harper dev-server aliases (opt-in; no identity override) |

`core` is the only layer that has to work everywhere, so it assumes nothing:
no Homebrew, no Node, no GUI. Every optional integration in it — starship, fzf,
the zsh plugins, git-delta — is guarded by a `command -v` check and degrades to
a sane built-in.

## Commands

```bash
dotfiles install [--profile NAME] [--with LAYER] [--without LAYER] [--dry-run] [--yes]
                 [--signing auto|ssh|gpg|off]
dotfiles list      # profiles and layers
dotfiles status    # what this machine has installed, and where links point
dotfiles link      # re-link $HOME files only, no packages
dotfiles doctor    # missing tools, broken links, key expiry
dotfiles github    # register ssh + gpg keys with GitHub
dotfiles key       # move the signing key between machines (export/import/status)
```

### Safety

Anything machine-global — `brew install`, `defaults write`, toolchain
installers — is skipped when `$HOME` is not your real home directory, because
that means a test harness with a sandboxed `$HOME`. File operations follow
`$HOME` and stay contained; these do not, and would escape the sandbox.
`DOTFILES_ALLOW_SANDBOXED_HOME=1` overrides it.

## How it works

```
layers/00-core/
├── home/          symlinked into $HOME
├── shell/*.sh     POSIX fragments — sourced by bash and zsh
├── zsh/*.zsh      zsh-only fragments
└── ssh/*.conf     ~/.ssh/config.d/ fragments
```

The numeric prefix sets both install order and shell-source order, and a test
asserts it matches the layer's declared `order`.

`~/.zshrc` is a three-line loader symlinked out of this repo. The real work
happens in `~/.config/dotfiles/init.sh`, generated at install time, which pins
`DOTFILES_ROOT` and lists each fragment as an explicit `source` line:

```sh
# --- core ---
. "$DOTFILES_ROOT/layers/00-core/shell/10-env.sh"
if [ -n "${ZSH_VERSION:-}" ]; then
  . "$DOTFILES_ROOT/layers/00-core/zsh/00-options.zsh"
fi
```

**Editing** a fragment takes effect in the next shell. **Adding** one means
re-running `dotfiles install` so it gets listed — cheap, since every step is a
no-op when already done.

Files in `$HOME` are symlinks back into the repo, so `vim ~/.zshrc` and
`git diff` here show the same edit. Anything already present is moved to
`~/.dotfiles-backup/<timestamp>/` first.

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
    order: 60,
    dir: "60-my-thing",
    description: "What it does.",
    platforms: ANY,          // or MACOS
    install: layers::my_thing::install,
},
```

Then create `layers/60-my-thing/` for its assets. It appears in `dotfiles list`
immediately, and the registry tests will fail if the order or directory prefix
disagree.

Helpers available: `ui::{head,step,ok,skip,warn,detail}`, `link::{link_file,
link_home_dir,write_file}`, `brew::bundle`, `sys::{has,which,is_macos,
is_container,allow_global_changes}`.

`Brewfile`s are a plain list, not `brew bundle` syntax — `formula <name>`,
`cask <name>`, `tap <name>`, one per line, `#` comments allowed. A cask that
has been renamed or pulled warns and the run continues.

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

- `~/.ssh/config.d/` fragments, symlinked from the repo. `~/.ssh/config` gets a
  single `Include ~/.ssh/config.d/*` line prepended (original backed up), so
  your own host entries stay where they are.
- `Host github.com` with `User git`, `AddKeysToAgent`, `UseKeychain` and the
  identity file. `IgnoreUnknown UseKeychain` keeps the same file working on
  Linux, where that option does not exist and would otherwise make ssh refuse
  to parse the config at all.
- Generates an ed25519 key if there isn't one (it prompts for a passphrase;
  unattended runs print the command instead of making a passphraseless key).
- Loads it into the agent, with `--apple-use-keychain` on macOS so it survives
  reboots.

`Host *` defaults live in `99-defaults.conf`, numbered last on purpose: ssh
takes the **first** value it sees for a keyword, so a `Host *` block placed
early silently overrides every specific host after it.

Git rewrites `https://github.com/` to `git@github.com:`, so every clone uses
SSH regardless of the URL you paste.

## Registering keys with GitHub

```bash
dotfiles github
```

GitHub treats these as three separate uploads, which is the part worth not
having to remember:

| What | Purpose | Token scope |
| --- | --- | --- |
| ssh **authentication** key | `git push` works | `admin:public_key` |
| ssh **signing** key | Verified badge (ssh signing) | `admin:ssh_signing_key` |
| gpg key | Verified badge (gpg signing) | `write:gpg_key` |

The same ssh key has to be uploaded **twice**, once as each type — that is the
step everyone forgets. `dotfiles github` checks what is already registered,
requests any missing token scope for you, uploads only what is needed, and then
verifies by authenticating over ssh and making a real signed commit in a temp
repo. Every upload asks first; nothing touches your account unattended.

## Commit signing

Chosen by `--signing` (or `$DOTFILES_SIGNING`), default `auto`:

| Mode | Behaviour |
| --- | --- |
| `auto` | the ssh key if there is one, else gpg key `79007EFD4678CB94`, else unsigned |
| `ssh` | force ssh signing — no gpg, no gpg-agent, no pinentry |
| `gpg` | force gpg |
| `off` | no signing |

**`auto` prefers ssh.** Every machine this repo sets up has an ssh key; it has
no agent, pinentry or expiry to manage, and it signs inside a devcontainer over
a forwarded agent with no extra setup. gpg is the fallback for a machine that
has the gpg key but no ssh key, and stays reachable with `--signing gpg`.

Neither mode is written into `~/.gitconfig` — both go to a generated include,
so a machine with no key at all gets working git rather than a git that fails
on every commit. Explicit `--signing ssh` with no ssh key present refuses
outright rather than writing a config that points at a missing file.

The catch with ssh signing: GitHub shows **Unverified** until the key is
uploaded a *second* time as a signing key. Pushes work either way, so it is
easy to miss — `dotfiles install` checks for this and tells you to run
`dotfiles github`.

SSH signing also writes `~/.ssh/allowed_signers`, without which git can create
signatures but not verify its own.

## Machines, containers and remote boxes

The rule: **the private key lives on laptops only.** Everything else borrows it
through the agent, or gets its own.

| Where | Auth (push) | Signing | What to do |
| --- | --- | --- | --- |
| Your laptops | own key each | own key each | `dotfiles install` |
| Devcontainer | host agent, forwarded | host agent, forwarded | `dotfiles install --profile core` inside |
| Remote box you use often | own key | own key | `dotfiles install --profile core` there |
| Remote box, one-off | forwarded agent | forwarded agent | `ssh -A`, scoped to that host |
| CI | deploy key / token | usually unsigned | out of scope for this repo |

### Why the agent is enough

`ssh-keygen -Y sign` will use a key held by the agent when the private key file
is not on disk. So a forwarded agent gives a container or remote box the
ability to both push *and* sign, while the private key never leaves your
laptop. Verified: with the private key file deleted and only the agent holding
it, `git commit` produces `%G? = G`.

This is the practical argument for ssh signing over gpg. Agent forwarding is
built into ssh and into VS Code's devcontainer and Remote-SSH support. Doing
the same with gpg means forwarding the gpg-agent socket via `RemoteForward`,
with matching gpg versions and `extra-socket` configured on both ends — it
works, but it is a thing you maintain.

### Devcontainers, specifically

VS Code already forwards your ssh agent and copies your `~/.gitconfig` into the
container. That gets you push and the right author identity — but **not
signing**, because the copied gitconfig's `[include]` lines point at
`~/.config/dotfiles/gitconfig.signing`, which does not exist in the container.
Git ignores missing includes silently, so commits come out unsigned with no
warning.

Fix it by running the core layer inside the container:

```jsonc
// .devcontainer/devcontainer.json
{
  "postCreateCommand": "curl -fsSL https://raw.githubusercontent.com/Ethan-Arrowood/dotfiles/main/install.sh | sh -s -- --profile core -y",
  "mounts": ["source=${localEnv:HOME}/.ssh/id_ed25519.pub,target=/tmp/id_ed25519.pub,type=bind,readonly"]
}
```

The core layer detects the container, leaves the login shell alone, skips key
generation, and configures signing against the forwarded agent. Only the
*public* key is mounted — the private key stays on the host.

### Remote servers

For a box you use regularly, give it its own key and run
`dotfiles install --profile core` on it. Per-machine keys mean revoking one
machine is one click in GitHub, with nothing else to rotate.

For a one-off, forward the agent — but scope it to that host, never globally:

```
# ~/.ssh/config.d/50-mybox.conf
Host mybox
    ForwardAgent yes
```

`ForwardAgent yes` under `Host *` would hand your agent to every server you
touch, and anyone with root on the far end can use it for as long as you are
connected. That is why this repo's `99-defaults.conf` does not set it.

## Moving the signing key to a new machine

**Do not put the private key in a repo or a gist.** A "secret" gist is
*unlisted*, not private — anyone with the URL reads it, no login needed, and
they get crawled. A private repo is readable by every token with `repo` scope
(CI, OAuth apps, agents) and git history keeps the file after you delete it.

Three options, in order of preference:

1. **Don't move it.** This is now the default — `auto` signs with the machine's
   own ssh key. Per-machine keys beat one portable key: nothing sensitive
   crosses the network, and revoking one machine is one click. The rest of this
   section is the fallback for when you specifically want gpg back.
2. **Password manager.** The key's own UID says `(Dashlane)` — store the
   armored export as a secure note. Purpose-built for exactly this.
3. **Encrypted file, stored anywhere.** If you want the convenience of pulling
   it down from a private repo or cloud storage:

   ```bash
   # old machine — asks for the key passphrase, then a new file passphrase
   dotfiles key export -o ~/gpg-signing-key.age

   # new machine
   dotfiles key import ~/gpg-signing-key.age
   dotfiles install --signing gpg
   ```

   Or skip the file entirely, since the key is already in your vault:

   ```bash
   dcli sync
   dotfiles key import --dashlane "GPG signing key"   # by title, or dl://<id>
   ```

   This refuses anything that is not a `PRIVATE KEY BLOCK`, which catches the
   common mistake of having stored only the public export.

   The output is age-encrypted (or `gpg --symmetric` if age isn't installed) on
   top of the passphrase already on the key, so the file is an opaque blob.
   *That* is safe in a private repo. Keep the file passphrase in your password
   manager, not next to the file.

`dotfiles key status` shows what signing material a machine currently has, and
`dotfiles doctor` warns when the gpg key is within 60 days of expiry (this one
expires 2028-07-19).
