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
    let installed = Cmd::new("xcode-select")
        .arg("-p")
        .capture()
        .map(|c| c.ok)
        .unwrap_or(false);

    if installed {
        // Present is the normal bar. Checking for a newer release costs a
        // network scan of ~15s, so it only happens when asked -- same rule as
        // `rustup update` and `uv self update`.
        if !ctx.update {
            ui::skip("xcode command line tools");
            return Ok(());
        }
        return update_clt(ctx);
    }
    if ctx.dry_run {
        ui::dry("install xcode command line tools");
        return Ok(());
    }

    let listing = with_clt_sentinel(|| Cmd::new("softwareupdate").arg("-l").capture())?;
    let label = newest_clt_label(&listing.combined());

    match label {
        Some(l) => {
            ui::step(&format!(
                "installing {l} (this takes a while; sudo will ask)"
            ));
            if !sys::as_root(
                ctx,
                &format!("softwareupdate --install {l}"),
                "softwareupdate",
                &["--install", &l, "--verbose"],
            )? {
                ui::warn("softwareupdate failed");
            }
        }
        None => {
            ui::warn("could not find a CLT package; falling back to the GUI installer");
            let _ = Cmd::new("xcode-select").arg("--install").capture();
            ui::warn("finish the GUI install, then re-run `dotfiles install`");
        }
    }
    Ok(())
}

/// Upgrade an installed CLT if `softwareupdate` offers a newer one.
fn update_clt(ctx: &Ctx) -> Result<()> {
    let Some(current) = installed_clt_version() else {
        ui::skip("xcode command line tools (installed; version not readable)");
        return Ok(());
    };
    if ctx.dry_run {
        ui::dry("check for a newer xcode command line tools");
        return Ok(());
    }

    ui::step("checking for a newer command line tools (scans software update)");
    let listing = with_clt_sentinel(|| Cmd::new("softwareupdate").arg("-l").capture())?;
    let Some(label) = newest_clt_label(&listing.combined()) else {
        ui::skip(&format!(
            "command line tools {current:?} is the newest offered"
        ));
        return Ok(());
    };

    // Compare on the leading components only: the label carries "27.0" while
    // pkgutil reports "26.6.0.0.1781586589".
    let available = version_key(&label);
    let n = current.len().min(available.len());
    if available[..n] <= current[..n] {
        ui::skip("xcode command line tools (up to date)");
        return Ok(());
    }

    ui::step(&format!("updating to {label}"));
    if !sys::as_root(
        ctx,
        &format!("softwareupdate --install {label}"),
        "softwareupdate",
        &["--install", &label, "--verbose"],
    )? {
        ui::warn("command line tools update failed; leaving the installed version alone");
    }
    Ok(())
}

/// The installed CLT version, from the package receipt.
fn installed_clt_version() -> Option<Vec<u32>> {
    let out = Cmd::new("pkgutil")
        .arg("--pkg-info=com.apple.pkg.CLTools_Executables")
        .capture()
        .ok()?;
    if !out.ok {
        return None;
    }
    let v = out
        .stdout
        .lines()
        .find_map(|l| l.strip_prefix("version:"))?
        .trim();
    Some(v.split('.').map(|n| n.parse().unwrap_or(0)).collect())
}

/// `softwareupdate` only lists CLT while this sentinel exists. Undocumented,
/// but verified on macOS 26.6. Removed again whatever the closure does.
fn with_clt_sentinel<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    let sentinel = "/tmp/.com.apple.dt.CommandLineTools.installondemand.in-progress";
    let created = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(sentinel)
        .is_ok();
    let result = f();
    if created {
        let _ = std::fs::remove_file(sentinel);
    }
    result
}

/// The highest-versioned "Command Line Tools" label in a `softwareupdate -l`
/// listing.
///
/// The listing is NOT version-ordered -- a real run on macOS 26.6 offered
/// 26.5, then 27.0, then 26.6, in that order -- so taking the last line picks
/// an older release than is available. Sort by the trailing version instead.
fn newest_clt_label(listing: &str) -> Option<String> {
    listing
        .lines()
        .filter_map(|l| l.split("Label:").nth(1))
        .map(str::trim)
        .filter(|l| l.starts_with("Command Line Tools"))
        .max_by_key(|l| version_key(l))
        .map(str::to_string)
}

/// Numeric sort key from the version at the end of a label, e.g.
/// "Command Line Tools for Xcode 27.0-27.0" -> [27, 0].
fn version_key(label: &str) -> Vec<u32> {
    label
        .rsplit('-')
        .next()
        .unwrap_or("")
        .split('.')
        .map(|n| n.parse().unwrap_or(0))
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim from a real `softwareupdate -l` on macOS 26.6.2, which lists
    /// 26.5, 27.0 and 26.6 in that order -- taking the last line gives 26.6.
    const LISTING: &str = "\
* Label: Safari27.0TahoeAuto-27.0
* Label: Command Line Tools for Xcode 26.5-26.5
* Label: Command Line Tools for Xcode 27.0-27.0
* Label: Command Line Tools for Xcode 26.6-26.6
* Label: macOS Tahoe 26.7.1-25G241
";

    #[test]
    fn picks_the_highest_version_not_the_last_line() {
        assert_eq!(
            newest_clt_label(LISTING).as_deref(),
            Some("Command Line Tools for Xcode 27.0-27.0")
        );
    }

    #[test]
    fn ignores_non_clt_labels_and_empty_listings() {
        assert_eq!(
            newest_clt_label("* Label: Safari27.0TahoeAuto-27.0\n"),
            None
        );
        assert_eq!(newest_clt_label(""), None);
    }

    /// pkgutil reports "26.6.0.0.1781586589" while the label carries "27.0",
    /// so only the leading components can be compared.
    #[test]
    fn installed_is_compared_against_the_label_on_leading_components() {
        let installed: Vec<u32> = "26.6.0.0.1781586589"
            .split('.')
            .map(|n| n.parse().unwrap_or(0))
            .collect();
        let newer = version_key("Command Line Tools for Xcode 27.0-27.0");
        let same = version_key("Command Line Tools for Xcode 26.6-26.6");
        let n = |a: &Vec<u32>| installed.len().min(a.len());

        assert!(newer[..n(&newer)] > installed[..n(&newer)], "27.0 > 26.6");
        assert!(
            same[..n(&same)] <= installed[..n(&same)],
            "26.6 is not newer"
        );
    }

    #[test]
    fn version_key_sorts_numerically_not_lexically() {
        assert!(version_key("x 9.0-9.0") < version_key("x 10.0-10.0"));
    }
}
