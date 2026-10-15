//! Run-wide context: where the repo is, and how destructive we are allowed
//! to be. Threaded through every layer instead of living in globals.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

pub struct Ctx {
    /// Root of the dotfiles repo (the directory holding `layers/`).
    pub root: PathBuf,
    pub home: PathBuf,
    pub dry_run: bool,
    pub assume_yes: bool,
    /// Refresh already-installed toolchains (rustup update, uv self update).
    /// Off by default so a plain re-run stays a no-op.
    pub update: bool,
    /// Configure commit signing. False only when --no-signing was passed.
    pub signing: bool,
    /// Where displaced files are moved. Created lazily on first use.
    pub backup_dir: PathBuf,
    /// Generated machine-local state: ~/.config/dotfiles
    pub state_dir: PathBuf,
}

impl Ctx {
    /// Everything here is either a location or a safe default. The flags are
    /// install-only and set by the caller.
    pub fn new(root: PathBuf) -> Result<Self> {
        let home = dirs::home_dir().context("cannot determine home directory")?;
        let stamp = std::process::Command::new("date")
            .args(["+%Y%m%d-%H%M%S"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "backup".into());

        // Deliberately NOT XDG_CONFIG_HOME-aware. layers/core/home/.gitconfig
        // has to name these include paths literally, and git does not expand
        // environment variables in them -- so honoring XDG here would silently
        // stop signing and delta from ever applying on a machine that sets it.
        // Tests override this field directly.
        let state_dir = home.join(".config").join("dotfiles");

        Ok(Self {
            backup_dir: home.join(".dotfiles-backup").join(stamp),
            state_dir,
            home,
            root,
            dry_run: false,
            assume_yes: false,
            update: false,
            signing: true,
        })
    }

    /// Locate the repo: an explicit override, else the directory above the
    /// running binary, else the current directory -- whichever first contains
    /// a `layers/` directory.
    pub fn find_root() -> Result<PathBuf> {
        if let Some(explicit) = std::env::var_os("DOTFILES_ROOT") {
            let p = PathBuf::from(explicit);
            if p.join("layers").is_dir() {
                // Absolute, always: symlink targets are stored verbatim, so a
                // relative root yields links that resolve against $HOME and
                // dangle -- after the real dotfiles have been displaced.
                return Ok(std::fs::canonicalize(&p).unwrap_or(p));
            }
            bail!("DOTFILES_ROOT={} has no layers/ directory", p.display());
        }

        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            // target/{debug,release}/dotfiles -> repo root, and bin/dotfiles -> repo root
            for up in [2usize, 3] {
                let mut p = exe.clone();
                for _ in 0..=up {
                    p.pop();
                }
                candidates.push(p);
            }
        }
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd);
        }
        // Where install.sh clones it. The binary lives in ~/.local/bin, which
        // is nowhere near the repo, so without this `dotfiles` only works from
        // a shell started after a successful install (which exports
        // DOTFILES_ROOT) -- and bash users never get that.
        if let Some(home) = dirs::home_dir() {
            candidates.push(home.join("dev/dotfiles"));
            candidates.push(home.join("dev/ethan-arrowood/dotfiles"));
        }

        for c in candidates {
            if c.join("layers").is_dir() {
                return Ok(std::fs::canonicalize(&c).unwrap_or(c));
            }
        }
        bail!("cannot find the dotfiles repo. Set DOTFILES_ROOT, or run from inside the checkout.")
    }

    /// `~`-shortened path for display.
    pub fn pretty(&self, p: &Path) -> String {
        match p.strip_prefix(&self.home) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => p.display().to_string(),
        }
    }
}
