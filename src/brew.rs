//! Parse `formula NAME`, `cask NAME` and `tap NAME` lines with `#` comments.
//! Avoid brew bundle syntax so bad entries stay non-fatal.

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
        let (Some(kind), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
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

/// Load brew variables so later steps find installed tools.
/// Evaluate `brew shellenv` in a shell: its path_helper calls and expansions
/// cannot be parsed as assignments. The old parser silently dropped PATH.
pub fn load_shellenv() -> Result<()> {
    let Some(brew) = brew_bin() else {
        return Ok(());
    };

    // NUL-separated so values containing newlines survive the round trip.
    let script = format!(
        "eval \"$({} shellenv)\" && exec /usr/bin/env -0",
        brew.to_string_lossy()
    );
    let out = Cmd::new("/bin/sh").args(["-c", &script]).capture()?;
    if !out.ok {
        ui::warn("could not evaluate `brew shellenv`; brew tools may not be found");
        return Ok(());
    }

    for entry in out.stdout.split('\0') {
        let Some((key, val)) = entry.split_once('=') else {
            continue;
        };
        // Import only brew variables, not /bin/sh's full environment.
        if matches!(
            key,
            "PATH"
                | "MANPATH"
                | "INFOPATH"
                | "FPATH"
                | "HOMEBREW_PREFIX"
                | "HOMEBREW_CELLAR"
                | "HOMEBREW_REPOSITORY"
        ) {
            std::env::set_var(key, val);
        }
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
    let sudo = sudo_check(sys::is_root(), sys::has_terminal());
    if ctx.dry_run {
        if sudo == SudoCheck::Prompt {
            ui::dry("sudo -v (asks for your password)");
        }
        ui::dry("install homebrew");
        return Ok(());
    }
    cache_sudo(ctx, sudo)?;
    ui::step("installing homebrew");
    // `bash -c "$(curl …)"` fails the same way a pipe does: if curl fails the
    // substitution is empty, bash runs nothing and exits 0.
    let ok = sys::run_remote_script(
        ctx,
        "homebrew installer",
        "https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh",
        "/bin/bash",
        &[],
        &[("NONINTERACTIVE", "1")],
    )?;
    if !ok {
        anyhow::bail!("homebrew install failed");
    }
    load_shellenv()?;
    Ok(())
}

/// How to get sudo credentials cached for Homebrew's installer. With
/// `NONINTERACTIVE=1` it only checks for cached credentials and never prompts.
#[derive(Debug, PartialEq, Eq)]
enum SudoCheck {
    /// Running as root.
    NotNeeded,
    /// Run `sudo -v` so the password prompt comes from sudo on the terminal.
    Prompt,
    /// No terminal to prompt on: only cached credentials can work.
    CachedOnly,
}

fn sudo_check(is_root: bool, has_terminal: bool) -> SudoCheck {
    match (is_root, has_terminal) {
        (true, _) => SudoCheck::NotNeeded,
        (false, true) => SudoCheck::Prompt,
        (false, false) => SudoCheck::CachedOnly,
    }
}

fn cache_sudo(ctx: &Ctx, check: SudoCheck) -> Result<()> {
    match check {
        SudoCheck::NotNeeded => Ok(()),
        SudoCheck::Prompt => {
            if !Cmd::new("sudo")
                .arg("-v")
                .run(ctx, "sudo -v (Homebrew's installer needs admin rights)")?
            {
                anyhow::bail!("sudo -v failed; installing Homebrew needs an admin account");
            }
            Ok(())
        }
        SudoCheck::CachedOnly => {
            // `-n` never prompts; it fails unless credentials are cached.
            let cached = Cmd::new("sudo")
                .args(["-n", "-v"])
                .capture()
                .is_ok_and(|c| c.ok);
            if !cached {
                anyhow::bail!(
                    "installing Homebrew needs sudo, and there is no terminal to ask \
                     for a password on. Run `sudo -v`, then rerun `dotfiles install`."
                );
            }
            Ok(())
        }
    }
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

/// Apps, package receipts and commands that identify an existing cask install.
/// Avoid replacing copies installed by hand or MDM.
#[derive(Debug, Default, PartialEq)]
struct CaskFootprint {
    apps: Vec<String>,
    pkg_ids: Vec<String>,
    commands: Vec<String>,
}

fn cask_footprint(name: &str) -> Option<CaskFootprint> {
    let out = brew_cmd()
        .args(["info", "--cask", "--json=v2", name])
        .capture()
        .ok()?;
    if !out.ok {
        return None;
    }
    let json: serde_json::Value = serde_json::from_str(&out.stdout).ok()?;
    Some(footprint(json["casks"].get(0)?))
}

fn footprint(cask: &serde_json::Value) -> CaskFootprint {
    // `app` and `pkgutil` are each either one string or a list of them.
    fn strings(v: &serde_json::Value) -> Vec<String> {
        match v {
            serde_json::Value::String(s) => vec![s.clone()],
            serde_json::Value::Array(a) => a.iter().flat_map(strings).collect(),
            _ => vec![],
        }
    }
    let mut f = CaskFootprint::default();
    for artifact in cask["artifacts"].as_array().into_iter().flatten() {
        f.apps.extend(strings(&artifact["app"]));
        // `["src", {"target": "agy"}]`: the command is the target if given,
        // else the source's file name.
        if let Some(b) = artifact["binary"].as_array() {
            let target = b.iter().find_map(|v| v["target"].as_str());
            let source = b.first().and_then(|v| v.as_str());
            if let Some(cmd) = target.or(source) {
                f.commands
                    .push(cmd.rsplit('/').next().unwrap_or(cmd).to_string());
            }
        }
        for step in artifact["uninstall"].as_array().into_iter().flatten() {
            f.pkg_ids.extend(strings(&step["pkgutil"]));
        }
    }
    f
}

/// Why a cask counts as installed without brew, if it does.
fn installed_outside_brew(name: &str) -> Option<String> {
    let f = cask_footprint(name)?;
    if let Some(app) = f
        .apps
        .iter()
        .find(|a| Path::new("/Applications").join(a).exists())
    {
        return Some(format!("/Applications/{app} already present"));
    }
    f.pkg_ids
        .iter()
        .find(|id| {
            Cmd::new("pkgutil")
                .args(["--pkg-info", id.as_str()])
                .capture()
                .is_ok_and(|c| c.ok)
        })
        .map(|id| format!("package {id} already installed"))
        .or_else(|| {
            f.commands
                .iter()
                .find_map(|c| sys::which(c).map(|p| format!("`{c}` already at {}", p.display())))
        })
}

// Check installed packages during dry-run so it lists only missing ones.

pub fn install_formula(ctx: &Ctx, name: &str) -> Result<()> {
    if has_formula(name) {
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
    if has_cask(name) {
        ui::skip(&format!("{name} (cask)"));
        return Ok(());
    }
    if let Some(why) = installed_outside_brew(name) {
        ui::skip(&format!("{name} ({why})"));
        return Ok(());
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
    if let Ok(listed) = brew_cmd().arg("tap").capture() {
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
    // Layers with Brewfiles can be selected without mac-base, which installs brew.
    if !ctx.dry_run && brew_bin().is_none() {
        ui::warn("Homebrew is missing; add the mac-base layer");
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
    fn extra_tokens_are_reported_not_dropped() {
        let (e, p) = parse("formula git delta\n");
        assert!(e.is_empty());
        assert_eq!(p.len(), 1, "{p:?}");
    }

    #[test]
    fn unknown_kinds_are_reported_with_line_numbers() {
        let (e, p) = parse("formula jq\nbrew ripgrep\n");
        assert_eq!(e, vec![Entry::Formula("jq".into())]);
        assert_eq!(p.len(), 1);
        assert!(p[0].contains("line 2"), "{}", p[0]);
    }

    /// Homebrew emits PATH via `eval "$(path_helper -s)"`, with no `export `
    /// prefix, so the old line-parser dropped it. Skipped when brew is absent.
    #[test]
    fn load_shellenv_imports_path() {
        let Some(brew) = brew_bin() else {
            return;
        };
        // Whichever prefix this machine has: /opt/homebrew, /usr/local, or
        // Linuxbrew on CI.
        let bin_dir = brew.parent().unwrap().to_string_lossy().into_owned();
        let before = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", "/usr/bin:/bin");
        load_shellenv().unwrap();
        let after = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", before);
        assert!(
            after.split(':').any(|d| d == bin_dir),
            "{bin_dir} missing from PATH after load_shellenv: {after}"
        );
    }

    #[test]
    fn cask_footprint_reads_apps_and_pkg_receipts() {
        let cask: serde_json::Value = serde_json::from_str(
            r#"{"artifacts":[
                {"uninstall":[{"quit":"x","pkgutil":"com.example.pkg"}]},
                {"pkg":["Example.pkg"]},
                {"app":["Example.app"]},
                {"uninstall":[{"pkgutil":["a.one","a.two"]}]},
                {"binary":["antigravity",{"target":"agy"}]},
                {"binary":["bin/codex"]}
            ]}"#,
        )
        .unwrap();
        assert_eq!(
            footprint(&cask),
            CaskFootprint {
                apps: vec!["Example.app".into()],
                pkg_ids: vec!["com.example.pkg".into(), "a.one".into(), "a.two".into()],
                commands: vec!["agy".into(), "codex".into()],
            }
        );
    }

    #[test]
    fn sudo_prompts_only_with_a_terminal_and_never_as_root() {
        assert_eq!(sudo_check(true, true), SudoCheck::NotNeeded);
        assert_eq!(sudo_check(true, false), SudoCheck::NotNeeded);
        assert_eq!(sudo_check(false, true), SudoCheck::Prompt);
        assert_eq!(sudo_check(false, false), SudoCheck::CachedOnly);
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
