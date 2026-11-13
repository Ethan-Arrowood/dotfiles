//! Essential applications, the Claude Code CLI, and the Ghostty config.

use crate::ctx::Ctx;
use crate::layer::Layer;
use crate::layers::mac_base::{current_default, normalized};
use crate::sys::{self, Cmd};
use crate::{brew, link, ui};
use anyhow::Result;
use std::path::Path;

pub fn install(ctx: &Ctx, layer: &Layer) -> Result<()> {
    let dir = layer.path(ctx);
    brew::bundle(ctx, &dir.join("Brewfile"))?;
    ghostty_config(ctx, &dir)?;
    claude_code(ctx)?;
    vscode_cli(ctx)?;
    chrome_defaults(ctx)?;
    default_browser(ctx)?;
    Ok(())
}

fn ghostty_config(ctx: &Ctx, layer_dir: &Path) -> Result<()> {
    let src = layer_dir.join("config/ghostty/config");
    if !src.is_file() {
        return Ok(());
    }
    let bin = Path::new("/Applications/Ghostty.app/Contents/MacOS/ghostty");
    if bin.is_file() && !ctx.dry_run {
        // Report validation errors without aborting installation.
        let out = Cmd::new(bin.to_string_lossy())
            .arg("+validate-config")
            .arg(format!("--config-file={}", src.display()))
            .capture()?;
        if out.ok {
            ui::ok("ghostty config validates");
        } else {
            ui::warn("ghostty config has problems (linking anyway; ghostty skips bad lines):");
            ui::detail(out.combined().trim());
        }
    }
    link::link_file(ctx, &src, &ctx.home.join(".config/ghostty/config"))
}

fn claude_code(ctx: &Ctx) -> Result<()> {
    if !ctx.dry_run && !sys::allow_global_changes() {
        return Ok(());
    }
    if sys::has("claude") {
        ui::skip("claude code cli");
        return Ok(());
    }
    if ctx.dry_run {
        ui::dry("install claude code cli");
        return Ok(());
    }
    if !sys::run_remote_script(
        ctx,
        "installing claude code cli",
        "https://claude.ai/install.sh",
        "/bin/bash",
        &[],
        &[],
    )? {
        ui::warn("claude code install failed");
    }
    Ok(())
}

/// `code` on PATH without opening VS Code to run the shell-command action.
fn vscode_cli(ctx: &Ctx) -> Result<()> {
    let bin = Path::new("/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code");
    if !bin.is_file() {
        return Ok(());
    }
    link::link_file(ctx, bin, &ctx.home.join(".local/bin/code"))
}

/// Set user Chrome policies. MDM-managed preferences take precedence.
fn chrome_defaults(ctx: &Ctx) -> Result<()> {
    if !ctx.dry_run && !sys::allow_global_changes() {
        return Ok(());
    }
    let policies: [(&str, &str, &str); 7] = [
        // Dashlane owns credentials; stop Chrome competing for the same job.
        ("PasswordManagerEnabled", "-bool", "false"),
        ("AutofillAddressEnabled", "-bool", "false"),
        ("AutofillCreditCardEnabled", "-bool", "false"),
        ("PromotionalTabsEnabled", "-bool", "false"),
        ("DefaultBrowserSettingEnabled", "-bool", "false"),
        ("MetricsReportingEnabled", "-bool", "false"),
        ("RestoreOnStartup", "-int", "1"),
    ];
    let mut changed = false;
    for (key, kind, value) in policies {
        // Read first, as the Defaultsfile does, so a second run writes nothing.
        if current_default("com.google.Chrome", key).as_deref()
            == Some(normalized(kind, value).as_str())
        {
            continue;
        }
        changed = true;
        Cmd::new("defaults")
            .args(["write", "com.google.Chrome", key, kind, value])
            .run(
                ctx,
                &format!("defaults write com.google.Chrome {key} {value}"),
            )?;
    }
    if changed {
        ui::detail(
            "Chrome policy changes need a full restart of Chrome; confirm at chrome://policy",
        );
    } else {
        ui::skip("Chrome policies already set");
    }
    Ok(())
}

const CHROME_APP: &str = "/Applications/Google Chrome.app";
/// Launch Services stores bundle ids in lowercase.
const CHROME_ID: &str = "com.google.chrome";
const LAUNCH_SERVICES: &str =
    "Library/Preferences/com.apple.LaunchServices/com.apple.launchservices.secure.plist";

/// Make Chrome the default browser when it isn't. macOS asks for confirmation
/// in a dialog, so this needs someone at the screen.
fn default_browser(ctx: &Ctx) -> Result<()> {
    if !ctx.dry_run && !sys::allow_global_changes() {
        return Ok(());
    }
    match chrome_is_default(&ctx.home.join(LAUNCH_SERVICES)) {
        Ok(true) => {
            ui::skip("Chrome is already the default browser");
            return Ok(());
        }
        Ok(false) => {}
        Err(e) => {
            ui::warn("could not read the default browser; leaving it unchanged");
            ui::detail(&e);
            return Ok(());
        }
    }
    if ctx.dry_run {
        ui::dry("make Chrome the default browser (macOS asks to confirm)");
        return Ok(());
    }
    if !sys::has_gui_session() {
        ui::skip("default browser: no GUI session to confirm the macOS dialog");
        return Ok(());
    }
    if !Path::new(CHROME_APP).is_dir() || !sys::has("defaultbrowser") {
        ui::warn("Chrome or defaultbrowser is missing; leaving the default browser unchanged");
        return Ok(());
    }
    // defaultbrowser sets both http and https.
    if !Cmd::new("defaultbrowser").arg("chrome").run(
        ctx,
        "making Chrome the default browser: confirm the macOS dialog",
    )? {
        ui::warn("defaultbrowser chrome failed");
    }
    Ok(())
}

/// Read the http and https handlers from the Launch Services plist. A
/// missing file or `LSHandlers` key means nobody chose a browser yet, so
/// Safari is the default.
fn chrome_is_default(plist: &Path) -> Result<bool, String> {
    if !plist.is_file() {
        return Ok(false);
    }
    let out = Cmd::new("plutil")
        .args(["-extract", "LSHandlers", "json", "-o", "-"])
        .arg(plist.to_string_lossy())
        .capture()
        .map_err(|e| e.to_string())?;
    if !out.ok {
        if out.stderr.contains("No value at that key path") {
            return Ok(false);
        }
        return Err(out.combined().trim().to_string());
    }
    handles_web(&out.stdout, CHROME_ID)
}

/// Does `bundle_id` handle both http and https in this `LSHandlers` JSON?
fn handles_web(json: &str, bundle_id: &str) -> Result<bool, String> {
    let handlers: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("LSHandlers is not JSON: {e}"))?;
    let Some(handlers) = handlers.as_array() else {
        return Err("LSHandlers is not an array".into());
    };
    let handler = |scheme: &str| {
        handlers
            .iter()
            .find(|h| h["LSHandlerURLScheme"].as_str() == Some(scheme))
            .and_then(|h| h["LSHandlerRoleAll"].as_str())
    };
    Ok(["http", "https"]
        .iter()
        .all(|s| handler(s).is_some_and(|id| id.eq_ignore_ascii_case(bundle_id))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(scheme: &str, id: &str) -> String {
        format!(
            r#"{{"LSHandlerURLScheme":"{scheme}","LSHandlerRoleAll":"{id}",
                "LSHandlerPreferredVersions":{{"LSHandlerRoleAll":"-"}}}}"#
        )
    }

    #[test]
    fn chrome_must_handle_http_and_https() {
        let other =
            r#"{"LSHandlerContentType":"public.html","LSHandlerRoleAll":"com.apple.safari"}"#;
        let both = format!(
            "[{other},{},{}]",
            entry("https", "com.google.chrome"),
            entry("http", "com.google.chrome")
        );
        assert_eq!(handles_web(&both, CHROME_ID), Ok(true));

        let one = format!(
            "[{},{}]",
            entry("https", "com.google.chrome"),
            entry("http", "com.apple.safari")
        );
        assert_eq!(handles_web(&one, CHROME_ID), Ok(false));

        let safari = format!(
            "[{},{}]",
            entry("https", "com.apple.safari"),
            entry("http", "com.apple.safari")
        );
        assert_eq!(handles_web(&safari, CHROME_ID), Ok(false));
        assert_eq!(handles_web("[]", CHROME_ID), Ok(false));
    }

    #[test]
    fn bundle_ids_compare_without_case() {
        let mixed = format!(
            "[{},{}]",
            entry("https", "com.google.Chrome"),
            entry("http", "COM.GOOGLE.CHROME")
        );
        assert_eq!(handles_web(&mixed, CHROME_ID), Ok(true));
    }

    #[test]
    fn unreadable_handlers_are_errors() {
        assert!(handles_web("not json", CHROME_ID).is_err());
        assert!(handles_web("{}", CHROME_ID).is_err());
    }

    #[test]
    fn a_missing_plist_means_safari() {
        let missing = Path::new("/nonexistent/com.apple.launchservices.secure.plist");
        assert_eq!(chrome_is_default(missing), Ok(false));
    }
}
