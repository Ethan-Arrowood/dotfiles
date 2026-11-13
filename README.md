# dotfiles

A Rust CLI that sets up my Macs, Linux containers and remote machines from layers.

On a fresh machine:

```bash
curl -fsSL https://raw.githubusercontent.com/Ethan-Arrowood/dotfiles/main/install.sh | sh -s -- --profile mac-dev
```

With `dotfiles` installed:

```bash
dotfiles install --profile mac-dev
```

On a fresh Mac, install Apple's Command Line Tools when prompted, then rerun the command. [`install.sh`](install.sh) clones or updates the repo and downloads a prebuilt binary. If the download fails or the platform needs a source build, it uses `cargo build` if available.

Use `--dry-run` (`-n`) to preview CLI changes. The bootstrap script still downloads the repo and binary. Repeated installs skip matching links, generated files, defaults and installed toolchains. `--update` refreshes rustup and uv. Software Update handles installed Xcode Command Line Tools.

See [ARCHITECTURE.md](ARCHITECTURE.md) for the install loop, file inventory, guards and tests.

## Profiles

Each Mac profile adds to the one above it. The default is `mac-dev` on macOS outside containers, otherwise `core`.

| Profile | Layers | For |
| --- | --- | --- |
| `core` | core | Linux devcontainers, CI images, remote boxes |
| `mac-minimal` | + mac-base, mac-apps | VMs and loaners |
| `mac-dev` | + mac-langs | Development |
| `mac-full` | + mac-apps-extra | Daily use |

Add or omit layers with `--with` and `--without`. You cannot omit `core`.

```bash
dotfiles install --profile mac-full --without mac-apps-extra
dotfiles install --profile mac-full --with work-harper
dotfiles install --profile core --with mac-apps
```

## Layers

| Layer | Platform | Contents |
| --- | --- | --- |
| `core` | any | zsh, git, ssh, commit signing, aliases, functions |
| `mac-base` | macOS | Xcode CLT, Homebrew, CLI tools, macOS defaults, keyboard and mouse shortcuts, Dock, computer name |
| `mac-langs` | macOS | Node (fnm, with one npm global prefix, `~/.npm-global`, shared by every version, and the globals in its [`Npmfile`](layers/mac-langs/Npmfile)), Rust (rustup), Python (uv), C/C++, Java |
| `mac-apps` | macOS | Chrome, VS Code, Claude, Ghostty, Dashlane CLI |
| `mac-apps-extra` | macOS | Slack, Zoom, Spotify, Discord, Podman, DaisyDisk |
| `work-harper` | any | [dispatch](src/layers/work_harper.rs) client and Claude plugin, Harper aliases and skills. On macOS: Codex, Antigravity (`agy`), gcloud, Tailscale, LastPass. Opt-in; no identity override |

`core` needs no Homebrew, Node or GUI. It skips optional shell integrations when absent. Other layers' Brewfiles need Homebrew; `mac-base` installs it.

## Commands

| Command | Purpose |
| --- | --- |
| `dotfiles install [--profile NAME] [--with LAYER] [--without LAYER] [--dry-run] [--yes] [--no-signing] [--update]` | Install selected layers. `--yes` (`-y`) accepts prompts |
| `dotfiles list` | List profiles and layers |
| `dotfiles status` | Show recorded layers and dotfile links |
| `dotfiles link` | Re-link `home/` files and reapply Skillfiles, without packages. Use `dotfiles install` for SSH and app links |
| `dotfiles doctor` | Check tools, SSH, signing and broken links |
| `dotfiles github` | Register and verify SSH authentication and signing keys |
| `dotfiles ncu` | Set up this machine's node-core-utils tokens |

Every command takes `--verbose` (`-v`). It prints the output of each failed clone attempt.

### Safety

The CLI skips machine-global changes when `$HOME` differs from the account's home or cannot be verified. `DOTFILES_ALLOW_SANDBOXED_HOME=1` overrides this guard. See [guarded operations](ARCHITECTURE.md#the-three-dangerous-operations).

## How it works

Layer assets link into `$HOME`. Editing a linked file also edits the repo. Replaced files go to `~/.dotfiles-backup/<timestamp>/`; real directories stay untouched. See [file handling](ARCHITECTURE.md#everything-that-touches-home).

Shell fragments load in layer order, then filename order. Edits take effect in the next shell. After adding a fragment, rerun `dotfiles install` to regenerate `~/.config/dotfiles/init.sh`.

Installers that append to `~/.zshrc` or `~/.zshenv` follow those links into this repo. Review unexpected diffs. This tool passes `--no-modify-path` to rustup and `INSTALLER_NO_MODIFY_PATH=1` to uv to prevent that.

## Adding a layer

Add a module under `src/layers/`:

```rust
// src/layers/my_thing.rs
pub fn install(ctx: &Ctx, layer: &Layer) -> Result<()> {
    brew::bundle(ctx, &layer.path(ctx).join("Brewfile"))?;
    Ok(())
}
```

Declare `pub mod my_thing;` in [`src/layers/mod.rs`](src/layers/mod.rs). Add this entry to `registry()` in [`src/layer.rs`](src/layer.rs), in install order:

```rust
Layer {
    name: "my-thing",
    description: "What it does.",
    macos_only: false,
    install: layers::my_thing::install,
},
```

Create `layers/my-thing/` for its assets. A test checks that it exists. The layer then appears in `dotfiles list`. See [asset formats and helpers](ARCHITECTURE.md#layer-assets).

## Agent config

[agentic](https://github.com/Ethan-Arrowood/agentic) owns the skills, rules and hooks. The [core](layers/core/Skillfile) and [work-harper](layers/work-harper/Skillfile) Skillfiles install them:

| Skillfile line | Result |
| --- | --- |
| `repo  Ethan-Arrowood/agentic` | Skills linked into `~/.claude/skills/` |
| `rules Ethan-Arrowood/agentic` | Global rules in `~/.claude/rules/`; org rules at `~/dev/<org>/AGENTS.md` where the org directory exists |
| `hook  claude Ethan-Arrowood/agentic#agents/claude/sync.sh SessionStart 15` | Sync hook in `~/.claude/settings.json`; runs once at install |
| `hook codex Ethan-Arrowood/agentic#agents/codex/sync.sh SessionStart` (work-harper) | Sync hook in `~/.codex/hooks.json` for `startup`; runs once at install to generate `~/.codex/AGENTS.md` and link `~/.agents/skills/` |

Trust new Codex hooks with `/hooks`. Start a new session to load changed rules. Claude Code skips symlinked user-scope rules in desktop Cowork sessions.

`~/.claude/CLAUDE.md` stays machine-local. The tool merges agent JSON files and preserves your existing plugin options. See [merge and org-rule handling](ARCHITECTURE.md#agent-config).

## Skills

| Skillfile syntax | Selects |
| --- | --- |
| `repo <owner>/<repo>` | Every skill |
| `skill <owner>/<repo>#<skill>` | One skill |
| `rules <owner>/<repo>` | Rules, separately from skills |
| `hook <agent> <owner>/<repo>#<path> <Event> [secs]` | A hook for `claude` or `codex` |

Repos clone to `$DOTFILES_SRC_DIR/<owner>/<repo>` (default `~/dev`), with lowercase owners. Cloning tries HTTPS, then `gh repo clone`, then SSH. When all three fail, the installer prints one warning with the last error. The installer leaves existing checkouts alone. agentic's sync hooks may run `git pull --ff-only`.

Skills link to those editable checkouts, so edits apply without syncing. The first layer claiming a skill name wins. Local skill directories stay untouched. Remove `harperfast/skills-internal` from the work Skillfile on machines not used for Harper contribution.

## Node.js core tools

`mac-langs` installs `@node-core/utils` (`ncu-ci`, `git-node`). Its config holds a GitHub token and a Jenkins token, so it stays out of this repo. Like the ssh key, each machine gets its own tokens:

```bash
dotfiles ncu
```

It sets your username from `gh`, then opens two pages. On GitHub, the new classic token already has `user:email` and `read:org` ticked and is named after the machine: pick an expiry, generate it, and paste it at the hidden prompt. On ci.nodejs.org, add an API token and paste that too. The config ends up at `~/.config/ncurc` (ncu reads `$XDG_CONFIG_HOME/ncurc`, which `.zshenv` sets), mode `0600`. An existing `~/.ncurc` moves there, with a link left at the old path for shells that don't set `XDG_CONFIG_HOME`. `dotfiles install` and `dotfiles doctor` point here while the config is missing.

## macOS shortcuts

`mac-base` applies [`Shortcuts.json`](layers/mac-base/Shortcuts.json), copied from a configured Mac. Apple doesn't document the ids. Names below come from community lists, or from what the binding does; the rest are marked unknown.

| Ids | Shortcut | State |
| --- | --- | --- |
| 15, 17, 19, 21, 23, 25, 26 | Accessibility zoom: on/off, zoom in, zoom out, invert colors, image smoothing, increase and decrease contrast | Off |
| 16, 18, 20, 22, 24 | Unknown, numbered among the zoom shortcuts | Off |
| 38 | Mouse button 5 (a side button): Mission Control. Inferred: it's the only binding for that button | On |
| 40 | Shift + mouse button 5. Unknown, probably Mission Control in slow motion | On |
| 60, 61 | Previous and next input source (⌃Space, ⌃⌥Space) | Off |
| 79, 80, 81, 82 | Move one Space left or right (⌃← and ⌃→), with their slow-motion variants | On |
| 164 | F5 with no modifiers. Unknown | On |
| 176 | Unknown | Off |

To name an unknown id, toggle a shortcut in System Settings > Keyboard > Keyboard Shortcuts and see which entry changes in `defaults read com.apple.symbolichotkeys AppleSymbolicHotKeys`.

## Machine-local escape hatches

| File | Purpose |
| --- | --- |
| `~/.zshrc.local` | Runs last in interactive zsh |
| `~/.zshenv.local` | Runs in every zsh before `init.sh`. Set `DOTFILES_NO_AUTOCD` and `HARPER_DEV_ROOT` here |
| `~/.zprofile.local` | Login shell settings |
| `~/.gitconfig.local` | Machine-specific Git settings |

## SSH

`core` links SSH fragments into `~/.ssh/config.d/` and prepends `Include ~/.ssh/config.d/*` to `~/.ssh/config`, backing up the original. The managed `github.com` settings take precedence; fallback defaults preserve your other host settings.

Outside containers, it creates a missing ed25519 key and prompts for a passphrase. Without a terminal, it prints the command instead. It loads the key into the agent, using `--apple-use-keychain` on macOS to save the passphrase.

`DOTFILES_SSH_KEY` overrides `~/.ssh/id_ed25519`. For SSH authentication with another path, also add a matching `IdentityFile` in `~/.ssh/config.d/`.

Git pushes `https://github.com/` URLs over SSH (`pushInsteadOf`). Clones and fetches keep their given URL.

## Registering keys with GitHub

```bash
dotfiles github
```

The command handles `gh auth login`, missing token scopes and uploads, asking before account changes. It checks SSH authentication and makes a signed commit in a temporary repo to verify local signing.

| Key type | Purpose | Token scope |
| --- | --- | --- |
| Authentication | `git push` | `admin:public_key` |
| Signing | Verified commits | `admin:ssh_signing_key` |

Upload the same public key as both types. A missing signing upload leaves commits Unverified even when pushes work. Configure signing with `dotfiles install` first.

## Commit signing

The machine's SSH key signs commits and tags. `install` configures signing when its public key is usable; otherwise it leaves signing unconfigured and prints setup steps. Use `--no-signing` to opt out.

Settings live in `~/.config/dotfiles/gitconfig.signing`. `~/.ssh/allowed_signers` enables local verification. The tool never reads private key material.

## Machines, containers and remote boxes

Keep each private key on its machine. Forward its agent or give another machine its own key.

| Where | Authentication and signing | Setup |
| --- | --- | --- |
| Laptop | Own key | `dotfiles install` |
| Devcontainer | Forwarded host agent | `dotfiles install --profile core` inside |
| Regular remote box | Own key | `dotfiles install --profile core` there |
| One-off remote box | Forwarded agent | `ssh -A`, scoped to that host |
| CI | Deploy key or token; usually unsigned | Outside this repo's scope |

### Devcontainers, specifically

VS Code forwards the SSH agent and copies `~/.gitconfig`, but its signing include is absent in a new container. Run core inside the container and mount the public key:

```jsonc
// .devcontainer/devcontainer.json
{
  "postCreateCommand": "curl -fsSL https://raw.githubusercontent.com/Ethan-Arrowood/dotfiles/main/install.sh | sh -s -- --profile core -y",
  "mounts": ["source=${localEnv:HOME}/.ssh/id_ed25519.pub,target=/tmp/id_ed25519.pub,type=bind,readonly"],
  "remoteEnv": { "DOTFILES_SSH_KEY": "/tmp/id_ed25519" }
}
```

`DOTFILES_SSH_KEY` names the private-key path; the installer appends `.pub`. Only mount the public key. Without this override, it looks for `~/.ssh/id_ed25519.pub` and leaves signing unconfigured if absent. Core skips key generation and login-shell changes in containers. The forwarded agent signs without a private key file in the container.

### Remote servers

Give regular remote machines their own keys so you can revoke each separately. For one-off access, scope forwarding to one host:

```text
# ~/.ssh/config.d/50-mybox.conf
Host mybox
    ForwardAgent yes
```

Never put `ForwardAgent yes` under `Host *`. Root on a remote server can use your forwarded agent while connected.

## The old GPG key (historical)

Commits were GPG-signed through 2026 with `79007EFD4678CB94` (RSA 4096, expires 2028-07-19). Dashlane holds the only copy of both halves. This tool no longer uses it.

For manual recovery, find the id with `dcli password "GPG signing key" -o json`, then import it:

```bash
dcli read 'dl://<secret-id>' | gpg --import
gpg --edit-key 79007EFD4678CB94 trust
```

Select `5` (ultimate), then quit.
