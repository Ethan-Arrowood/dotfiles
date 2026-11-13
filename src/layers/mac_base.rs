//! macOS base: developer tooling that everything else assumes.

use crate::ctx::Ctx;
use crate::layer::Layer;
use crate::sys::{self, Cmd};
use crate::{brew, link, skills, ui};
use anyhow::{Context, Result};
use std::path::Path;

pub fn install(ctx: &Ctx, layer: &Layer) -> Result<()> {
    xcode_clt(ctx)?;
    brew::ensure_installed(ctx)?;
    ui::step("installing base packages");
    brew::bundle(ctx, &layer.path(ctx).join("Brewfile"))?;
    ui::step("applying macOS defaults");
    macos_defaults(ctx, layer)?;
    shortcuts(ctx, layer)?;
    git_delta(ctx)?;
    computer_name(ctx)?;
    Ok(())
}

fn xcode_clt(ctx: &Ctx) -> Result<()> {
    if !ctx.dry_run && !sys::allow_global_changes() {
        return Ok(());
    }
    // Software Update handles CLT versions once installed.
    if Cmd::new("xcode-select")
        .arg("-p")
        .capture()
        .map(|c| c.ok)
        .unwrap_or(false)
    {
        ui::skip("xcode command line tools");
        return Ok(());
    }
    // Use Apple's installer. Headless softwareupdate installation is undocumented.
    if ctx.dry_run {
        ui::dry("open Apple's Command Line Tools installer");
        return Ok(());
    }
    let _ = Cmd::new("xcode-select").arg("--install").capture();
    ui::warn(
        "install the Command Line Tools in the window that opened, then re-run `dotfiles install`",
    );
    Ok(())
}

const HOTKEYS_DOMAIN: &str = "com.apple.symbolichotkeys";
const HOTKEYS_KEY: &str = "AppleSymbolicHotKeys";

/// Apply `Shortcuts.json`: entries of `com.apple.symbolichotkeys`, keyed by
/// macOS's shortcut id. Each entry is merged with `-dict-add`, so ids the file
/// doesn't name keep this machine's value. macOS stores only shortcuts that
/// differ from its defaults, so exporting that dict captures your changes.
fn shortcuts(ctx: &Ctx, layer: &Layer) -> Result<()> {
    let path = layer.path(ctx).join("Shortcuts.json");
    if !path.is_file() {
        return Ok(());
    }
    if !ctx.dry_run && !sys::allow_global_changes() {
        return Ok(());
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let want: serde_json::Map<String, serde_json::Value> = match serde_json::from_str(&text) {
        Ok(serde_json::Value::Object(m)) => m,
        _ => {
            ui::warn("Shortcuts.json is not a JSON object of shortcut ids; skipping it");
            return Ok(());
        }
    };
    let have = current_hotkeys();
    let mut changed = 0;
    for (id, entry) in &want {
        if have.get(id) == Some(entry) {
            continue;
        }
        changed += 1;
        Cmd::new("defaults")
            .args(["write", HOTKEYS_DOMAIN, HOTKEYS_KEY, "-dict-add", id])
            .arg(plist_xml(entry))
            .run(ctx, &format!("set keyboard/mouse shortcut {id}"))?;
    }
    if changed == 0 {
        ui::skip("shortcuts already set");
        return Ok(());
    }
    if !ctx.dry_run {
        // Makes the change live without logging out.
        let _ = Cmd::new(
            "/System/Library/PrivateFrameworks/SystemAdministration.framework/Resources/activateSettings",
        )
        .arg("-u")
        .capture();
        ui::ok(&format!("{changed} shortcut(s) changed"));
    }
    Ok(())
}

/// This machine's shortcut entries, as JSON. Empty when none are set.
fn current_hotkeys() -> serde_json::Map<String, serde_json::Value> {
    let Ok(plist) = Cmd::new("defaults")
        .args(["export", HOTKEYS_DOMAIN, "-"])
        .capture()
    else {
        return Default::default();
    };
    let tmp = std::env::temp_dir().join(format!("dotfiles-hotkeys-{}.plist", std::process::id()));
    if std::fs::write(&tmp, &plist.stdout).is_err() {
        return Default::default();
    }
    let json = Cmd::new("plutil")
        .args(["-extract", HOTKEYS_KEY, "json", "-o", "-"])
        .arg(tmp.to_string_lossy())
        .capture();
    let _ = std::fs::remove_file(&tmp);
    match json.ok().and_then(|c| serde_json::from_str(&c.stdout).ok()) {
        Some(serde_json::Value::Object(m)) => m,
        _ => Default::default(),
    }
}

/// A JSON value as the XML plist fragment `defaults write` accepts, which
/// keeps booleans and integers typed (an old-style plist string would not).
fn plist_xml(v: &serde_json::Value) -> String {
    use serde_json::Value;
    let esc = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    match v {
        Value::Null => "<string></string>".into(),
        Value::Bool(true) => "<true/>".into(),
        Value::Bool(false) => "<false/>".into(),
        Value::Number(n) if n.is_i64() || n.is_u64() => format!("<integer>{n}</integer>"),
        Value::Number(n) => format!("<real>{n}</real>"),
        Value::String(s) => format!("<string>{}</string>", esc(s)),
        Value::Array(a) => format!(
            "<array>{}</array>",
            a.iter().map(plist_xml).collect::<String>()
        ),
        Value::Object(m) => format!(
            "<dict>{}</dict>",
            m.iter()
                .map(|(k, v)| format!("<key>{}</key>{}", esc(k), plist_xml(v)))
                .collect::<String>()
        ),
    }
}

/// One line of the Defaultsfile.
#[derive(Debug, PartialEq)]
struct Setting {
    domain: String,
    key: String,
    kind: String,
    value: String,
}

const KINDS: [&str; 4] = ["-bool", "-int", "-float", "-string"];

/// Parse a Defaultsfile: `domain key type value`, `#` comments. A leading
/// `~/` in the value expands to $HOME. Malformed lines are reported with
/// their line number rather than skipped silently.
fn parse_defaults(text: &str, home: &Path) -> (Vec<Setting>, Vec<String>) {
    let mut out = Vec::new();
    let mut problems = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = skills::strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 4 || !KINDS.contains(&f[2]) {
            problems.push(format!(
                "line {}: expected `domain key {} value`: {line}",
                n + 1,
                KINDS.join("|")
            ));
            continue;
        }
        let value = f[3..].join(" ");
        let valid = match f[2] {
            "-bool" => ["true", "false", "YES", "NO", "1", "0"].contains(&value.as_str()),
            "-int" => value.parse::<i64>().is_ok(),
            "-float" => value.parse::<f64>().is_ok(),
            _ => true,
        };
        if !valid {
            problems.push(format!("line {}: `{value}` is not a valid {}", n + 1, f[2]));
            continue;
        }
        let value = match value.strip_prefix("~/") {
            Some(rest) => home.join(rest).to_string_lossy().into_owned(),
            None => value,
        };
        out.push(Setting {
            domain: f[0].into(),
            key: f[1].into(),
            kind: f[2].into(),
            value,
        });
    }
    (out, problems)
}

/// Write changed defaults only. Unconditional writes restarted the Dock and
/// closed Finder windows on every run.
fn macos_defaults(ctx: &Ctx, layer: &Layer) -> Result<()> {
    if !ctx.dry_run && !sys::allow_global_changes() {
        return Ok(());
    }
    let path = layer.path(ctx).join("Defaultsfile");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let (settings, problems) = parse_defaults(&text, &ctx.home);
    for p in &problems {
        ui::warn(&format!("Defaultsfile: {p}"));
    }

    let mut changed = 0;
    for s in &settings {
        if current_default(&s.domain, &s.key).as_deref()
            == Some(normalized(&s.kind, &s.value).as_str())
        {
            continue;
        }
        changed += 1;
        Cmd::new("defaults")
            .args(["write", &s.domain, &s.key, &s.kind, &s.value])
            .run(
                ctx,
                &format!("defaults write {} {} {}", s.domain, s.key, s.value),
            )?;
    }

    if changed == 0 {
        ui::skip("macOS defaults already set");
        return Ok(());
    }
    if !ctx.dry_run {
        for app in ["Finder", "Dock", "SystemUIServer"] {
            let _ = Cmd::new("killall").arg(app).capture();
        }
        ui::ok(&format!(
            "{changed} default(s) changed; restarted Finder/Dock"
        ));
        ui::detail("trackpad, mouse, appearance and keyboard changes apply at next login");
    }
    Ok(())
}

/// `defaults read` for one key, or None when unset.
pub(crate) fn current_default(domain: &str, key: &str) -> Option<String> {
    let out = Cmd::new("defaults")
        .args(["read", domain, key])
        .capture()
        .ok()?;
    if !out.ok {
        return None;
    }
    Some(out.trimmed().to_string())
}

/// Normalize booleans to the 0/1 form printed by `defaults read`.
pub(crate) fn normalized(kind: &str, value: &str) -> String {
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

    #[test]
    fn plist_xml_keeps_types() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"enabled":true,"value":{"parameters":[16,16,0],"type":"button"}}"#,
        )
        .unwrap();
        assert_eq!(
            plist_xml(&v),
            "<dict><key>enabled</key><true/><key>value</key><dict><key>parameters</key>\
             <array><integer>16</integer><integer>16</integer><integer>0</integer></array>\
             <key>type</key><string>button</string></dict></dict>"
        );
    }

    /// The shipped file must be an object of shortcut entries keyed by id.
    #[test]
    fn shipped_shortcuts_parse() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/layers/mac-base/Shortcuts.json"
        );
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let m = v.as_object().expect("an object");
        assert!(!m.is_empty());
        for (id, entry) in m {
            assert!(id.parse::<u32>().is_ok(), "id {id}");
            assert!(entry.get("enabled").is_some_and(|e| e.is_boolean()), "{id}");
        }
    }

    #[test]
    fn the_shipped_defaultsfile_parses_cleanly() {
        let text = include_str!("../../layers/mac-base/Defaultsfile");
        let (settings, problems) = parse_defaults(text, Path::new("/home/me"));
        assert!(problems.is_empty(), "{problems:?}");
        assert!(settings.len() > 30);
        let loc = settings.iter().find(|s| s.key == "location").unwrap();
        assert_eq!(loc.value, "/home/me/Downloads");
    }

    #[test]
    fn defaults_parser_strips_comments_and_reports_bad_lines() {
        let (s, p) = parse_defaults(
            "# heading\n\
             com.apple.dock  tilesize  -float  64   # note\n\
             com.apple.dock  tilesize  64\n\
             NSGlobalDomain  AppleInterfaceStyle  -str  Dark\n",
            Path::new("/h"),
        );
        assert_eq!(
            s,
            vec![Setting {
                domain: "com.apple.dock".into(),
                key: "tilesize".into(),
                kind: "-float".into(),
                value: "64".into(),
            }]
        );
        assert_eq!(p.len(), 2, "{p:?}");
        assert!(p[0].starts_with("line 3"), "{p:?}");
    }
}
