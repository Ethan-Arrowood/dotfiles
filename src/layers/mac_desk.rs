//! The desk's two-monitor layout. `restore-displays` applies it at login and on
//! wake, because the dock sometimes brings the monitors up rotated, at the
//! wrong resolution, or as generic displays.

use crate::ctx::Ctx;
use crate::layer::Layer;
use crate::sys::{self, Cmd};
use crate::{brew, link, ui};
use anyhow::Result;
use std::path::{Path, PathBuf};

const SCRIPT: &str = "restore-displays";
const LOGIN_LABEL: &str = "com.ethanarrowood.restore-displays";
const WAKE_LABEL: &str = "com.ethanarrowood.restore-displays-wake";

pub fn install(ctx: &Ctx, layer: &Layer) -> Result<()> {
    let dir = layer.path(ctx);
    brew::bundle(ctx, &dir.join("Brewfile"))?;
    let script = ctx.home.join(".local/bin").join(SCRIPT);
    link::link_file(ctx, &dir.join("bin").join(SCRIPT), &script)?;

    let Some(sleepwatcher) = sleepwatcher_bin() else {
        if ctx.dry_run {
            ui::dry("write and load the restore-displays LaunchAgents once sleepwatcher installs");
        } else {
            ui::warn("sleepwatcher is missing: skipping the restore-displays LaunchAgents");
        }
        return Ok(());
    };
    let agents = [
        (LOGIN_LABEL, login_agent(&script)),
        (WAKE_LABEL, wake_agent(&sleepwatcher, &script)),
    ];
    for (label, plist) in agents {
        let path = ctx
            .home
            .join("Library/LaunchAgents")
            .join(format!("{label}.plist"));
        let changed = std::fs::read_to_string(&path).ok().as_deref() != Some(plist.as_str());
        link::write_file(ctx, &path, &plist)?;
        load(ctx, label, &path, changed)?;
    }
    Ok(())
}

fn sleepwatcher_bin() -> Option<PathBuf> {
    let prefix = brew::brew_bin()?.parent()?.parent()?.to_path_buf();
    let bin = prefix.join("sbin/sleepwatcher");
    bin.is_file().then_some(bin)
}

/// Load the agent into the GUI session, reloading it when its plist changed.
/// Loading the login agent runs it once, which applies the layout.
fn load(ctx: &Ctx, label: &str, path: &Path, changed: bool) -> Result<()> {
    if !ctx.dry_run && !sys::allow_global_changes() {
        return Ok(());
    }
    if !sys::has_gui_session() {
        ui::skip(&format!("{label}: no GUI session; it loads at next login"));
        return Ok(());
    }
    let uid = Cmd::new("id").arg("-u").capture()?.trimmed().to_string();
    let domain = format!("gui/{uid}");
    let target = format!("{domain}/{label}");
    let loaded = Cmd::new("launchctl").args(["print", &target]).capture()?.ok;
    if loaded && !changed {
        ui::skip(&format!("{label} (loaded)"));
        return Ok(());
    }
    if loaded {
        Cmd::new("launchctl")
            .args(["bootout", &target])
            .run(ctx, &format!("launchctl bootout {target}"))?;
    }
    let path = path.to_string_lossy();
    if !Cmd::new("launchctl")
        .args(["bootstrap", &domain, &path])
        .run(ctx, &format!("launchctl bootstrap {label}"))?
    {
        ui::warn(&format!("could not load {label}"));
    }
    Ok(())
}

/// Runs once per login. 12 attempts, 10 s apart, give the dock 2 minutes.
fn login_agent(script: &Path) -> String {
    plist(LOGIN_LABEL, &[&script.to_string_lossy(), "12", "10"], false)
}

/// sleepwatcher stays running and runs its `-w` command through `sh -c` on
/// each wake. 6 attempts, 5 s apart.
fn wake_agent(sleepwatcher: &Path, script: &Path) -> String {
    let command = format!("{} 6 5", sh_quote(&script.to_string_lossy()));
    plist(
        WAKE_LABEL,
        &[&sleepwatcher.to_string_lossy(), "-w", &command],
        true,
    )
}

fn plist(label: &str, args: &[&str], keep_alive: bool) -> String {
    let args: String = args
        .iter()
        .map(|a| format!("    <string>{}</string>\n", xml_escape(a)))
        .collect();
    let keep_alive = if keep_alive {
        "  <key>KeepAlive</key>\n  <true/>\n"
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
{args}  </array>
  <key>RunAtLoad</key>
  <true/>
{keep_alive}</dict>
</plist>
"#
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_agent_runs_the_script_with_retries() {
        let p = login_agent(Path::new("/Users/me/.local/bin/restore-displays"));
        assert!(p.contains("<string>com.ethanarrowood.restore-displays</string>"));
        assert!(p.contains(
            "<string>/Users/me/.local/bin/restore-displays</string>\n    <string>12</string>\n    <string>10</string>"
        ));
        assert!(
            !p.contains("KeepAlive"),
            "a login agent must exit after one run"
        );
    }

    #[test]
    fn wake_agent_quotes_the_script_for_sleepwatchers_shell() {
        let p = wake_agent(
            Path::new("/opt/homebrew/sbin/sleepwatcher"),
            Path::new("/Users/O'Neil & co/.local/bin/restore-displays"),
        );
        assert!(p.contains("<string>-w</string>"));
        assert!(p.contains(
            r"<string>'/Users/O'\''Neil &amp; co/.local/bin/restore-displays' 6 5</string>"
        ));
        assert!(p.contains("<key>KeepAlive</key>"));
    }

    #[test]
    fn shipped_plists_are_valid() {
        if !sys::is_macos() {
            return;
        }
        let dir = crate::tmpdir::PrivateTempDir::new("mac-desk").unwrap();
        for (name, body) in [
            ("login.plist", login_agent(Path::new("/x/restore-displays"))),
            (
                "wake.plist",
                wake_agent(
                    Path::new("/x/sleepwatcher"),
                    Path::new("/x/restore-displays"),
                ),
            ),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, body).unwrap();
            let out = Cmd::new("plutil")
                .args(["-lint", &path.to_string_lossy()])
                .capture()
                .unwrap();
            assert!(out.ok, "{name}: {}", out.combined());
        }
    }

    #[test]
    fn script_is_executable() {
        use std::os::unix::fs::PermissionsExt;
        let script =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("layers/mac-desk/bin/restore-displays");
        let mode = std::fs::metadata(script).unwrap().permissions().mode();
        assert!(mode & 0o111 != 0, "restore-displays must be executable");
    }
}
