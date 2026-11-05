//! Shared paths and install flags, passed through layers instead of globals.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

pub struct Ctx {
    /// Root of the dotfiles repo (the directory holding `layers/`).
    pub root: PathBuf,
    pub home: PathBuf,
    pub dry_run: bool,
    pub assume_yes: bool,
    /// Refresh installed toolchains only with --update, keeping reruns unchanged.
    pub update: bool,
    /// Configure commit signing. False only when --no-signing was passed.
    pub signing: bool,
    /// Where displaced files are moved. Created lazily on first use.
    pub backup_dir: PathBuf,
    /// Generated machine-local state: ~/.config/dotfiles
    pub state_dir: PathBuf,
}

impl Ctx {
    /// Create paths and default flags. Only `install` overrides the flags.
    pub fn new(root: PathBuf) -> Result<Self> {
        let home = dirs::home_dir().context("cannot determine home directory")?;
        let stamp = std::process::Command::new("date")
            .args(["+%Y%m%d-%H%M%S"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "backup".into());

        // Git includes in layers/core/home/.gitconfig cannot expand XDG_CONFIG_HOME.
        // Keep state at their literal paths or signing and delta silently stop working.
        let state_dir = Self::state_dir_for(&home);

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

    /// Find `layers/` via DOTFILES_ROOT, the binary, cwd, or recorded shell init.
    pub fn find_root() -> Result<PathBuf> {
        if let Some(explicit) = std::env::var_os("DOTFILES_ROOT") {
            let p = PathBuf::from(explicit);
            if p.join("layers").is_dir() {
                // Relative symlink targets resolve against $HOME and dangle after replacing files.
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
        // Later shells may lack DOTFILES_ROOT. Read the last install location instead
        // of guessing. install.sh sets DOTFILES_ROOT for the first run.
        if let Some(recorded) = recorded_root() {
            candidates.push(recorded);
        }

        for c in candidates {
            if c.join("layers").is_dir() {
                return Ok(std::fs::canonicalize(&c).unwrap_or(c));
            }
        }
        bail!("cannot find the dotfiles repo. Set DOTFILES_ROOT, or run from inside the checkout.")
    }

    /// Where the generated shell init lives, without needing a full Ctx.
    fn state_dir_for(home: &Path) -> PathBuf {
        home.join(".config").join("dotfiles")
    }

    /// Expand a leading `~/` against $HOME.
    pub fn expand(&self, p: &str) -> PathBuf {
        match p.strip_prefix("~/") {
            Some(rest) => self.home.join(rest),
            None => PathBuf::from(p),
        }
    }

    /// `~`-shortened path for display.
    pub fn pretty(&self, p: &Path) -> String {
        match p.strip_prefix(&self.home) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => p.display().to_string(),
        }
    }
}

/// Undo `shellinit::sh_quote`: `'it'\''s'` -> `it's`.
fn unquote(v: &str) -> String {
    match v.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
        Some(inner) => inner.replace("'\\''", "'"),
        None => v.trim_matches('"').to_string(),
    }
}

fn recorded_root() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let text = std::fs::read_to_string(Ctx::state_dir_for(&home).join("init.sh")).ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix("export DOTFILES_ROOT="))
        .map(|v| unquote(v.trim()))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unquote_reverses_sh_quote() {
        let q = crate::shellinit::sh_quote("/it's/here");
        assert_eq!(unquote(&q), "/it's/here");
        assert_eq!(unquote("\"/plain\""), "/plain");
    }
}
