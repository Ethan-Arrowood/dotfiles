# dotfiles

Machine setup composed from layers. One repo covers a Linux devcontainer and a
fresh MacBook; you pick how much of it applies.

```bash
# fresh machine, nothing installed
curl -fsSL https://raw.githubusercontent.com/Ethan-Arrowood/dotfiles/main/install.sh | bash -s -- --profile mac-dev

# already cloned
./bin/dotfiles install --profile mac-dev
```

Always safe to run twice. Add `--dry-run` to see the plan without touching anything.

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
| `work-harper` | any | Harper git identity and dev-server aliases (opt-in) |

`core` is the only layer that has to work everywhere, so it assumes nothing:
no Homebrew, no Node, no GUI. Every optional integration in it — starship, fzf,
the zsh plugins, git-delta — is guarded by a `command -v` check and degrades to
a sane built-in.

## Commands

```bash
dotfiles install [--profile NAME] [--with LAYER] [--without LAYER] [--dry-run] [--yes]
dotfiles list      # profiles and layers
dotfiles status    # what this machine has installed, and where links point
dotfiles link      # re-link $HOME files only, no packages
dotfiles doctor    # check for problems (missing tools, broken links, key expiry)
```

## How it works

```
layers/00-core/
├── layer.conf     name, description, supported platforms
├── install.sh     idempotent install steps
├── home/          symlinked into $HOME
├── shell/*.sh     POSIX fragments — sourced by bash and zsh
└── zsh/*.zsh      zsh-only fragments
```

The numeric prefix sets both install order and shell-source order.

`~/.zshrc` is a three-line loader symlinked out of this repo. The real work
happens in `~/.config/dotfiles/init.sh`, generated at install time, which pins
`DOTFILES_ROOT` for the machine and lists the enabled layers' fragments as
explicit `source` lines:

```sh
# --- core ---
. "$DOTFILES_ROOT/layers/00-core/shell/10-env.sh"
if [ -n "${ZSH_VERSION:-}" ]; then
  . "$DOTFILES_ROOT/layers/00-core/zsh/00-options.zsh"
fi
```

Nothing is copied or concatenated, so **editing** a fragment takes effect in the
next shell. **Adding** a new fragment file means re-running `dotfiles install`
so it gets listed — cheap, since every step is a no-op when already done.

The list is generated rather than globbed at runtime for a reason: zsh doesn't
word-split unquoted variables the way bash does, and it aborts on an unmatched
glob — while the obvious guard for that, `setopt local_options null_glob`,
silently reverts every `setopt` a sourced fragment performs. Explicit lines
sidestep both, and a full four-layer startup measures ~0.05s.

Files placed in `$HOME` are symlinks back into the repo, so `vim ~/.zshrc` and
`git diff` in this repo show the same edit. Anything already there is moved to
`~/.dotfiles-backup/<timestamp>/` first.

## Adding a layer

```bash
mkdir -p layers/60-thing/{home,shell}
cat > layers/60-thing/layer.conf <<'EOF'
NAME=thing
DESCRIPTION=What it does.
PLATFORMS=any
EOF
```

Drop an `install.sh` (sourced with `$LAYER_DIR` set, plus `log_*`, `run`,
`link_file`, `write_file`, `brew_bundle`, `has`), a `Brewfile`, and shell
fragments as needed. It shows up in `dotfiles list` immediately.

`Brewfile`s are plain lists, not `brew bundle` syntax — `formula <name>`,
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

## Commit signing

The core layer configures GPG signing only when key `79007EFD4678CB94` is
already in the machine's keyring. If it isn't, git stays unsigned and the
installer prints the import steps — a container without the key gets working
git rather than a git that fails on every commit.

The key ID is committed; the private key never is. To move it to a new machine:

```bash
# on the machine that has it
gpg --export-secret-keys --armor 79007EFD4678CB94 > key.asc
# on the new machine
gpg --import key.asc && rm key.asc
gpg --edit-key 79007EFD4678CB94 trust   # 5 (ultimate), then quit
dotfiles install --profile mac-dev      # picks it up and enables signing
```

`dotfiles doctor` warns when the key is within 60 days of expiry (this one
expires 2028-07-19).
