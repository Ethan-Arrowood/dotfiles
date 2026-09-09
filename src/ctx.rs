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
    /// Where displaced files are moved. Created lazily on first use.
    pub backup_dir: PathBuf,
    /// Generated machine-local state: ~/.config/dotfiles
    pub state_dir: PathBuf,
}

impl Ctx {
    pub fn new(root: PathBuf, dry_run: bool, assume_yes: bool) -> Result<Self> {
        Self::with_update(root, dry_run, assume_yes, false)
    }

    pub fn with_update(
        root: PathBuf,
        dry_run: bool,
        assume_yes: bool,
        update: bool,
    ) -> Result<Self> {
        let home = dirs::home_dir().context("cannot determine home directory")?;
        let stamp = std::process::Command::new("date")
            .args(["+%Y%m%d-%H%M%S"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "backup".into());

        let state_dir = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join("dotfiles");

        Ok(Self {
            backup_dir: home.join(".dotfiles-backup").join(stamp),
            state_dir,
            home,
            root,
            dry_run,
            assume_yes,
            update,
        })
    }

    /// Locate the repo: an explicit override, else the directory above the
    /// running binary, else the current directory -- whichever first contains
    /// a `layers/` directory.
    pub fn find_root() -> Result<PathBuf> {
        if let Some(explicit) = std::env::var_os("DOTFILES_ROOT") {
            let p = PathBuf::from(explicit);
            if p.join("layers").is_dir() {
                return Ok(p);
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
        for c in candidates {
            if c.join("layers").is_dir() {
                return Ok(c);
            }
        }
        bail!("cannot find the dotfiles repo; set DOTFILES_ROOT or run from inside it")
    }

    /// `~`-shortened path for display.
    pub fn pretty(&self, p: &Path) -> String {
        match p.strip_prefix(&self.home) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => p.display().to_string(),
        }
    }
}
