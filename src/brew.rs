//! Homebrew helpers and Brewfile parsing.
//!
//! Brewfiles here are a plain list -- `formula NAME`, `cask NAME`, `tap NAME`,
//! one per line, `#` comments allowed. Not `brew bundle` syntax, so there is
//! no dependency on the bundle tap and a single bad entry stays non-fatal.

use crate::ctx::Ctx;
use crate::sys::{self, Cmd};
use crate::ui;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub enum Entry {
    Formula(String),
    Cask(String),
    Tap(String),
}

/// Parse a Brewfile. Unknown kinds are reported with their line number rather
/// than silently skipped.
pub fn parse(text: &str) -> (Vec<Entry>, Vec<String>) {
    let mut entries = Vec::new();
    let mut problems = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        // A trailing comment must not become part of the package name.
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let (Some(kind), Some(name)) = (parts.next(), parts.next()) else {
            problems.push(format!(
                "line {}: expected `<kind> <name>`, got `{line}`",
                i + 1
            ));
            continue;
        };
        match kind {
            "formula" => entries.push(Entry::Formula(name.to_string())),
            "cask" => entries.push(Entry::Cask(name.to_string())),
            "tap" => entries.push(Entry::Tap(name.to_string())),
            other => problems.push(format!("line {}: unknown kind `{other}`", i + 1)),
        }
    }
    (entries, problems)
}

pub fn brew_bin() -> Option<PathBuf> {
    for candidate in [
        "/opt/homebrew/bin/brew",
        "/usr/local/bin/brew",
        "/home/linuxbrew/.linuxbrew/bin/brew",
    ] {
        let p = PathBuf::from(candidate);
        if p.is_file() {
            return Some(p);
        }
    }
    sys::which("brew")
}

/// Put brew's environment into this process so later steps find the packages
/// it installs without a new shell.
pub fn load_shellenv() -> Result<()> {
    let Some(brew) = brew_bin() else {
        return Ok(());
    };
    let out = Cmd::new(brew.to_string_lossy()).arg("shellenv").capture()?;
    if !out.ok {
        return Ok(());
    }
    for line in out.stdout.lines() {
        // export KEY="value";
        let Some(rest) = line.strip_prefix("export ") else {
            continue;
        };
        let Some((key, val)) = rest.split_once('=') else {
            continue;
        };
        let val = val.trim_end_matches(';').trim_matches('"');
        std::env::set_var(key, val);
    }
    Ok(())
}

pub fn ensure_installed(ctx: &Ctx) -> Result<()> {
    if !ctx.dry_run && !sys::allow_global_changes() {
        return Ok(());
    }
    if brew_bin().is_some() {
        load_shellenv()?;
        ui::skip("homebrew (already installed)");
        return Ok(());
    }
    if ctx.dry_run {
        ui::dry("install homebrew");
        return Ok(());
    }
    ui::step("installing homebrew");
    let script = "/bin/bash -c \"$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\"";
    let ok = Cmd::new("/bin/bash")
        .args(["-c", script])
        .run(ctx, "homebrew installer")?;
    if !ok {
        anyhow::bail!("homebrew install failed");
    }
    load_shellenv()?;
    Ok(())
}

fn brew_cmd() -> Cmd {
    Cmd::new(
        brew_bin()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "brew".into()),
    )
}

fn has_formula(name: &str) -> bool {
    brew_cmd()
        .args(["list", "--formula", "--versions", name])
        .capture()
        .map(|c| c.ok)
        .unwrap_or(false)
}

fn has_cask(name: &str) -> bool {
    brew_cmd()
        .args(["list", "--cask", "--versions", name])
        .capture()
        .map(|c| c.ok)
        .unwrap_or(false)
}

/// The .app a cask installs, so a hand- or MDM-installed copy is not clobbered.
fn cask_app_name(name: &str) -> Option<String> {
    let out = brew_cmd().args(["info", "--cask", name]).capture().ok()?;
    if !out.ok {
        return None;
    }
    out.stdout.lines().find_map(|l| {
        let l = l.trim();
        l.strip_suffix(" (App)").map(|s| s.to_string())
    })
}

pub fn install_formula(ctx: &Ctx, name: &str) -> Result<()> {
    if !ctx.dry_run && has_formula(name) {
        ui::skip(name);
        return Ok(());
    }
    if !brew_cmd()
        .args(["install", name])
        .run(ctx, &format!("brew install {name}"))?
    {
        ui::warn(&format!("brew install {name} failed"));
    }
    Ok(())
}

pub fn install_cask(ctx: &Ctx, name: &str) -> Result<()> {
    if !ctx.dry_run {
        if has_cask(name) {
            ui::skip(&format!("{name} (cask)"));
            return Ok(());
        }
        if let Some(app) = cask_app_name(name) {
            if Path::new("/Applications").join(&app).exists() {
                ui::skip(&format!("{name} (/Applications/{app} already present)"));
                return Ok(());
            }
        }
    }
    if !brew_cmd()
        .args(["install", "--cask", name])
        .run(ctx, &format!("brew install --cask {name}"))?
    {
        ui::warn(&format!("cask {name} failed (renamed or unavailable?)"));
    }
    Ok(())
}

pub fn tap(ctx: &Ctx, name: &str) -> Result<()> {
    if !ctx.dry_run {
        let listed = brew_cmd().arg("tap").capture()?;
        if listed.stdout.lines().any(|l| l.trim() == name) {
            ui::skip(&format!("tap {name}"));
            return Ok(());
        }
    }
    if !brew_cmd()
        .args(["tap", name])
        .quiet()
        .run(ctx, &format!("brew tap {name}"))?
    {
        ui::warn(&format!("tap {name} failed"));
    }
    Ok(())
}

/// Install everything in a layer's Brewfile. Taps run first so a namespaced
/// formula in the same file resolves.
pub fn bundle(ctx: &Ctx, path: &Path) -> Result<()> {
    if !ctx.dry_run && !sys::allow_global_changes() {
        return Ok(());
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let (entries, problems) = parse(&text);
    for p in &problems {
        ui::warn(&format!(
            "{}: {p}",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
    }
    for e in entries.iter() {
        if let Entry::Tap(name) = e {
            tap(ctx, name)?;
        }
    }
    for e in entries.iter() {
        match e {
            Entry::Formula(name) => install_formula(ctx, name)?,
            Entry::Cask(name) => install_cask(ctx, name)?,
            Entry::Tap(_) => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_comments_are_not_part_of_the_name() {
        let (e, p) = parse("formula gh      # GitHub CLI\ncask slack\n");
        assert_eq!(
            e,
            vec![Entry::Formula("gh".into()), Entry::Cask("slack".into())]
        );
        assert!(p.is_empty());
    }

    #[test]
    fn blank_and_comment_lines_are_ignored() {
        let (e, p) = parse("\n# just a comment\n\n   \nformula jq\n");
        assert_eq!(e, vec![Entry::Formula("jq".into())]);
        assert!(p.is_empty());
    }

    #[test]
    fn unknown_kinds_are_reported_with_line_numbers() {
        let (e, p) = parse("formula jq\nbrew ripgrep\n");
        assert_eq!(e, vec![Entry::Formula("jq".into())]);
        assert_eq!(p.len(), 1);
        assert!(p[0].contains("line 2"), "{}", p[0]);
    }

    #[test]
    fn namespaced_formulae_survive() {
        let (e, _) = parse("tap dashlane/tap\nformula dashlane/tap/dashlane-cli\n");
        assert_eq!(
            e,
            vec![
                Entry::Tap("dashlane/tap".into()),
                Entry::Formula("dashlane/tap/dashlane-cli".into())
            ]
        );
    }
}
