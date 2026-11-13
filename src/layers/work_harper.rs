//! Harper tools and aliases. Keep ethan@arrowood.dev as the Git identity:
//! an owned domain preserves attribution after an employer address expires.

use crate::ctx::Ctx;
use crate::layer::Layer;
use crate::sys::{self, Cmd};
use crate::{brew, link, skills, ui};
use anyhow::Result;
use std::path::Path;

pub fn install(ctx: &Ctx, layer: &Layer) -> Result<()> {
    let brewfile = layer.path(ctx).join("Brewfile");
    if sys::is_macos() {
        brew::bundle(ctx, &brewfile)?;
        tailscale(ctx);
    } else {
        ui::skip("not macOS: skipping codex, agy, gcloud, Tailscale, LastPass and screen");
    }
    dispatch(ctx)?;
    if !ctx.home.join("dev/harperfast").is_dir() {
        ui::warn("~/dev/harperfast does not exist yet; the harper aliases assume that layout");
    }
    Ok(())
}

/// Report Tailscale status. Joining needs manual sign-in; auth keys are secrets.
fn tailscale(ctx: &Ctx) {
    let app = std::path::Path::new("/Applications/Tailscale.app");
    let cli = sys::which("tailscale").or_else(|| {
        let p = app.join("Contents/MacOS/Tailscale");
        p.is_file().then_some(p)
    });
    let Some(cli) = cli else {
        if !ctx.dry_run {
            ui::detail("Tailscale is not installed; it comes from this layer's Brewfile");
        }
        return;
    };
    let state = Cmd::new(cli.to_string_lossy())
        .args(["status", "--json"])
        .capture()
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c.stdout).ok())
        .and_then(|v| v["BackendState"].as_str().map(String::from));
    match state.as_deref() {
        Some("Running") => ui::skip("tailscale connected"),
        _ => ui::detail(
            "Tailscale disconnected. Open it, allow its VPN configuration if asked, and sign in with your work account",
        ),
    }
}

/// Run dispatch from its checkout and register the Claude plugin.
/// Leave the secret token setup manual.
fn dispatch(ctx: &Ctx) -> Result<()> {
    let Some(checkout) = skills::ensure_clone(ctx, "harperFast", "dispatch")? else {
        return Ok(());
    };
    link::link_file(
        ctx,
        &checkout.join("bin/dispatch"),
        &ctx.home.join(".local/bin/dispatch"),
    )?;
    if !sys::has("node") {
        ui::warn("dispatch needs node on PATH (mac-langs installs it)");
    }
    claude_plugin(ctx, &checkout)?;

    let env_file = ctx.home.join(".config/dispatch/worker.env");
    if !env_file.is_file() && std::env::var_os("DISPATCH_WORKER_TOKEN").is_none() {
        ui::detail(&format!(
            "dispatch has no token: put DISPATCH_WORKER_TOKEN=<id:token> in {} (chmod 600), then run `dispatch doctor`",
            ctx.pretty(&env_file)
        ));
    }
    Ok(())
}

fn claude_plugin(ctx: &Ctx, checkout: &Path) -> Result<()> {
    if !ctx.dry_run && !sys::allow_global_changes() {
        return Ok(());
    }
    if !sys::has("claude") {
        ui::skip("claude not on PATH: skipping the dispatch plugin");
        return Ok(());
    }
    // Strip JSON whitespace so formatting changes do not trigger duplicate registration.
    let listed = |args: &[&str], needle: &str| -> Result<bool> {
        let out = Cmd::new("claude").args(args.iter().copied()).capture()?;
        let flat: String = out.stdout.split_whitespace().collect();
        Ok(out.ok && flat.contains(needle))
    };

    if listed(
        &["plugin", "marketplace", "list", "--json"],
        r#""name":"dispatch""#,
    )? {
        ui::skip("dispatch marketplace (already added)");
    } else if !Cmd::new("claude")
        .args(["plugin", "marketplace", "add"])
        .arg(checkout.to_string_lossy())
        .run(ctx, "adding the dispatch plugin marketplace")?
    {
        ui::warn("could not add the dispatch marketplace");
        return Ok(());
    }

    if listed(&["plugin", "list", "--json"], r#""id":"dispatch@dispatch""#)? {
        ui::skip("dispatch plugin (already installed)");
    } else if !Cmd::new("claude")
        .args(["plugin", "install", "dispatch@dispatch"])
        .run(ctx, "installing the dispatch plugin")?
    {
        ui::warn("could not install the dispatch plugin");
    }
    Ok(())
}
