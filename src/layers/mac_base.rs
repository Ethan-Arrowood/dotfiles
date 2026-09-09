//! macOS base: developer tooling that everything else assumes.

use crate::ctx::Ctx;
use crate::layer::Layer;
use crate::sys::{self, Cmd};
use crate::{brew, link, ui};
use anyhow::Result;

pub fn install(ctx: &Ctx, layer: &Layer) -> Result<()> {
    xcode_clt(ctx)?;
    brew::ensure_installed(ctx)?;
    ui::step("installing base packages");
    brew::bundle(ctx, &layer.path(ctx).join("Brewfile"))?;
    ui::step("applying macOS defaults");
    macos_defaults(ctx)?;
    git_delta(ctx)?;
    computer_name(ctx)?;
    Ok(())
}

fn xcode_clt(ctx: &Ctx) -> Result<()> {
    if !ctx.dry_run && !sys::allow_global_changes() {
        return Ok(());
    }
    if Cmd::new("xcode-select")
        .arg("-p")
        .capture()
        .map(|c| c.ok)
        .unwrap_or(false)
    {
        ui::skip("xcode command line tools");
        return Ok(());
    }
    if ctx.dry_run {
        ui::dry("install xcode command line tools");
        return Ok(());
    }

    // Apple looks for this exact path, so it cannot be moved somewhere private.
    // create_new rather than create: the path is world-writable and
    // File::create would follow a planted symlink.
    let sentinel = "/tmp/.com.apple.dt.CommandLineTools.installondemand.in-progress";
    let sentinel_created = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(sentinel)
        .is_ok();

    let listing = Cmd::new("softwareupdate").arg("-l").capture()?;
    let label = listing
        .combined()
        .lines()
        .filter(|l| l.contains("Label:") && l.contains("Command Line Tools"))
        .filter_map(|l| l.split("Label:").nth(1).map(|s| s.trim().to_string()))
        .next_back();

    match label {
        Some(l) => {
            ui::step(&format!("installing {l} (this takes a while)"));
            if !Cmd::new("softwareupdate")
                .args(["--install", &l, "--verbose"])
                .run(ctx, "softwareupdate")?
            {
                ui::warn("softwareupdate failed");
            }
        }
        None => {
            ui::warn("could not find a CLT package; falling back to the GUI installer");
            let _ = Cmd::new("xcode-select").arg("--install").capture();
            ui::warn("finish the GUI install, then re-run `dotfiles install`");
        }
    }
    if sentinel_created {
        let _ = std::fs::remove_file(sentinel);
    }
    Ok(())
}

/// Only the settings worth losing an argument over. Anything cosmetic is left
/// to System Settings, where it is easier to change your mind.
///
/// Reads each key before writing it: writing unconditionally meant every run
/// ended in `killall Finder Dock SystemUIServer`, restarting the Dock and
/// closing open Finder windows even when nothing had changed.
fn macos_defaults(ctx: &Ctx) -> Result<()> {
    if !ctx.dry_run && !sys::allow_global_changes() {
        return Ok(());
    }
    let home_downloads = ctx.home.join("Downloads");
    let mut settings: Vec<(&str, &str, &str, String)> = vec![
        (
            "NSGlobalDomain",
            "ApplePressAndHoldEnabled",
            "-bool",
            "false".into(),
        ),
        ("NSGlobalDomain", "KeyRepeat", "-int", "2".into()),
        ("NSGlobalDomain", "InitialKeyRepeat", "-int", "15".into()),
        (
            "NSGlobalDomain",
            "AppleShowAllExtensions",
            "-bool",
            "true".into(),
        ),
        (
            "NSGlobalDomain",
            "NSDocumentSaveNewDocumentsToCloud",
            "-bool",
            "false".into(),
        ),
        (
            "com.apple.finder",
            "AppleShowAllFiles",
            "-bool",
            "true".into(),
        ),
        ("com.apple.finder", "ShowPathbar", "-bool", "true".into()),
        ("com.apple.finder", "ShowStatusBar", "-bool", "true".into()),
        (
            "com.apple.finder",
            "FXPreferredViewStyle",
            "-string",
            "Nlsv".into(),
        ),
        (
            "com.apple.finder",
            "_FXSortFoldersFirst",
            "-bool",
            "true".into(),
        ),
        (
            "com.apple.finder",
            "FXDefaultSearchScope",
            "-string",
            "SCcf".into(),
        ),
        (
            "com.apple.desktopservices",
            "DSDontWriteNetworkStores",
            "-bool",
            "true".into(),
        ),
        (
            "com.apple.screencapture",
            "disable-shadow",
            "-bool",
            "true".into(),
        ),
        ("com.apple.dock", "show-recents", "-bool", "false".into()),
        ("com.apple.dock", "autohide-delay", "-float", "0".into()),
    ];
    settings.push((
        "com.apple.screencapture",
        "location",
        "-string",
        home_downloads.to_string_lossy().into_owned(),
    ));

    let mut changed = Vec::new();
    for (domain, key, kind, value) in &settings {
        if current_default(domain, key).as_deref() == Some(normalized(kind, value).as_str()) {
            continue;
        }
        changed.push(format!("{domain} {key}"));
        Cmd::new("defaults")
            .args(["write", domain, key, kind])
            .arg(value.clone())
            .run(ctx, &format!("defaults write {domain} {key} {value}"))?;
    }

    if changed.is_empty() {
        ui::skip("macOS defaults already set");
        return Ok(());
    }
    if !ctx.dry_run {
        // Only restart the UI when something actually moved.
        for app in ["Finder", "Dock", "SystemUIServer"] {
            let _ = Cmd::new("killall").arg(app).capture();
        }
        ui::ok(&format!(
            "{} default(s) changed; restarted Finder/Dock",
            changed.len()
        ));
    }
    Ok(())
}

/// `defaults read` for one key, or None when unset.
fn current_default(domain: &str, key: &str) -> Option<String> {
    let out = Cmd::new("defaults")
        .args(["read", domain, key])
        .capture()
        .ok()?;
    if !out.ok {
        return None;
    }
    Some(out.trimmed().to_string())
}

/// `defaults read` prints booleans as 0/1 and floats without a trailing
/// fraction, so compare in that form rather than against the literal we pass.
fn normalized(kind: &str, value: &str) -> String {
    match kind {
        "-bool" => match value {
            "true" | "YES" | "1" => "1".into(),
            _ => "0".into(),
        },
        _ => value.to_string(),
    }
}

/// Wire up git-delta only when it is actually installed.
fn git_delta(ctx: &Ctx) -> Result<()> {
    let out = ctx.state_dir.join("gitconfig.tools");
    if !sys::has("delta") {
        link::remove_if_present(ctx, &out)?;
        ui::skip("git-delta not installed; leaving the default pager");
        return Ok(());
    }
    let body = "# Generated by `dotfiles install` because git-delta is installed.\n\
                [core]\n\tpager = delta\n\
                [interactive]\n\tdiffFilter = delta --color-only\n\
                [delta]\n\tnavigate = true\n\tline-numbers = true\n\tside-by-side = false\n\thyperlinks = true\n";
    link::write_file(ctx, &out, body)
}

fn computer_name(ctx: &Ctx) -> Result<()> {
    if !ctx.dry_run && !sys::allow_global_changes() {
        return Ok(());
    }
    let current = Cmd::new("scutil")
        .args(["--get", "ComputerName"])
        .capture()
        .ok()
        .filter(|c| c.ok)
        .map(|c| c.trimmed().to_string())
        .unwrap_or_default();
    ui::step(&format!("computer name is currently '{current}'"));

    let requested = std::env::var("DOTFILES_COMPUTER_NAME")
        .ok()
        .filter(|s| !s.is_empty());
    let name = match requested {
        Some(n) => n,
        None => {
            // Non-interactive runs never rename the machine.
            if ctx.assume_yes || ctx.dry_run {
                ui::skip("computer name unchanged (set DOTFILES_COMPUTER_NAME to rename)");
                return Ok(());
            }
            match prompt_line(&format!(
                "New computer name (blank to keep \"{current}\"): "
            )) {
                Some(n) if !n.trim().is_empty() => n.trim().to_string(),
                _ => {
                    ui::skip("computer name unchanged");
                    return Ok(());
                }
            }
        }
    };
    if name == current {
        ui::skip(&format!("computer name already '{name}'"));
        return Ok(());
    }

    // LocalHostName and NetBIOSName accept alphanumerics and hyphens only.
    let safe: String = name
        .chars()
        .map(|c| if c == ' ' { '-' } else { c })
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    if safe.is_empty() {
        ui::warn(&format!(
            "'{name}' has no characters valid in a hostname; leaving the name unchanged"
        ));
        return Ok(());
    }

    sys::as_root(
        ctx,
        &format!("ComputerName  = {name}"),
        "scutil",
        &["--set", "ComputerName", &name],
    )?;
    sys::as_root(
        ctx,
        &format!("HostName      = {safe}"),
        "scutil",
        &["--set", "HostName", &safe],
    )?;
    sys::as_root(
        ctx,
        &format!("LocalHostName = {safe}"),
        "scutil",
        &["--set", "LocalHostName", &safe],
    )?;
    sys::as_root(
        ctx,
        &format!("NetBIOSName   = {safe}"),
        "defaults",
        &[
            "write",
            "/Library/Preferences/SystemConfiguration/com.apple.smb.server",
            "NetBIOSName",
            "-string",
            &safe,
        ],
    )?;
    Ok(())
}

fn prompt_line(question: &str) -> Option<String> {
    use std::io::{BufRead, Write};
    let mut tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok()?;
    let _ = write!(tty, "    ? {question}");
    let _ = tty.flush();
    let mut line = String::new();
    std::io::BufReader::new(tty).read_line(&mut line).ok()?;
    Some(line)
}
