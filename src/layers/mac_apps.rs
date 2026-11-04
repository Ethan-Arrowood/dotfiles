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
    Ok(())
}

/// Ghostty reads a plain text config, so it can be a dotfile like anything else.
fn ghostty_config(ctx: &Ctx, layer_dir: &Path) -> Result<()> {
    let src = layer_dir.join("config/ghostty/config");
    if !src.is_file() {
        return Ok(());
    }
    let bin = Path::new("/Applications/Ghostty.app/Contents/MacOS/ghostty");
    if bin.is_file() && !ctx.dry_run {
        // +validate-config exits 1 on a bad config. Capture rather than run so
        // a typo'd option cannot abort the install.
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

/// Chrome policies in the user preference domain. An MDM-pushed managed
/// preference always wins over these, which is the correct precedence on a
/// work machine: these fill gaps, they do not fight IT.
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
