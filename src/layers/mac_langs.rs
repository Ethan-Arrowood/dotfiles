//! Language toolchains. Each installer is skipped when already present.

use crate::ctx::Ctx;
use crate::layer::Layer;
use crate::sys::{self, Cmd};
use crate::{brew, link, ui};
use anyhow::Result;

pub fn install(ctx: &Ctx, layer: &Layer) -> Result<()> {
    let dir = layer.path(ctx);
    ui::step("installing toolchain packages");
    brew::bundle(ctx, &dir.join("Brewfile"))?;
    brew::load_shellenv()?;
    node(ctx)?;
    rust(ctx)?;
    python(ctx)?;
    link::link_home_dir(ctx, &dir)?;
    Ok(())
}

fn node(ctx: &Ctx) -> Result<()> {
    if !ctx.dry_run && !sys::allow_global_changes() {
        return Ok(());
    }
    if !sys::has("fnm") {
        ui::warn("fnm missing; skipping Node install");
        return Ok(());
    }
    let listed = Cmd::new("fnm").arg("list").capture()?;
    if listed.stdout.contains('v')
        && listed
            .stdout
            .lines()
            .any(|l| l.contains("v1") || l.contains("v2"))
    {
        ui::skip("fnm already has Node versions installed");
        return Ok(());
    }
    for args in [["install", "--lts"], ["install", "--latest"]] {
        if !Cmd::new("fnm")
            .args(args)
            .run(ctx, &format!("fnm {}", args.join(" ")))?
        {
            ui::warn(&format!("fnm {} failed", args.join(" ")));
        }
    }
    let _ = Cmd::new("fnm")
        .args(["default", "lts-latest"])
        .run(ctx, "fnm default lts-latest");
    Ok(())
}

fn rust(ctx: &Ctx) -> Result<()> {
    if !ctx.dry_run && !sys::allow_global_changes() {
        return Ok(());
    }
    if sys::has("rustup") {
        if !ctx.update {
            ui::skip("rustup already installed (pass --update to refresh)");
            return Ok(());
        }
        if !Cmd::new("rustup")
            .args(["update", "stable"])
            .run(ctx, "rustup update stable")?
        {
            ui::warn("rustup update failed");
        }
        return Ok(());
    }
    if ctx.dry_run {
        ui::dry("install rustup");
        return Ok(());
    }
    ui::step("installing rustup");
    // --no-modify-path: this repo owns the shell config, and rustup would
    // otherwise append to ~/.zshenv -- which is a symlink into the repo, so the
    // edit would land in tracked content.
    if !sys::run_remote_script(
        ctx,
        "rustup-init",
        "https://sh.rustup.rs",
        "/bin/sh",
        &["-y", "--no-modify-path"],
        &[],
    )? {
        ui::warn("rustup install failed");
        return Ok(());
    }
    let cargo_bin = ctx.home.join(".cargo/bin");
    if let Some(path) = std::env::var_os("PATH") {
        let mut paths = vec![cargo_bin.clone()];
        paths.extend(std::env::split_paths(&path));
        if let Ok(joined) = std::env::join_paths(paths) {
            std::env::set_var("PATH", joined);
        }
    }
    let _ = Cmd::new("rustup")
        .args(["component", "add", "rust-analyzer", "clippy", "rustfmt"])
        .run(ctx, "rustup component add rust-analyzer clippy rustfmt");
    Ok(())
}

fn python(ctx: &Ctx) -> Result<()> {
    if !ctx.dry_run && !sys::allow_global_changes() {
        return Ok(());
    }
    if sys::has("uv") {
        ui::skip("uv already installed");
        let _ = Cmd::new("uv")
            .args(["self", "update"])
            .run(ctx, "uv self update");
        return Ok(());
    }
    if ctx.dry_run {
        ui::dry("install uv");
        return Ok(());
    }
    ui::step("installing uv");
    // Same reasoning as rustup above. uv's installer is cargo-dist based, which
    // reads INSTALLER_NO_MODIFY_PATH; newer versions also honor
    // UV_NO_MODIFY_PATH. Without this it appends to ~/.zshenv, and since that
    // is a symlink into this repo the line lands in tracked content -- which
    // has already happened once here.
    if !sys::run_remote_script(
        ctx,
        "uv installer",
        "https://astral.sh/uv/install.sh",
        "/bin/sh",
        &[],
        &[
            ("INSTALLER_NO_MODIFY_PATH", "1"),
            ("UV_NO_MODIFY_PATH", "1"),
        ],
    )? {
        ui::warn("uv install failed");
        return Ok(());
    }
    let local_bin = ctx.home.join(".local/bin");
    if let Some(path) = std::env::var_os("PATH") {
        let mut paths = vec![local_bin];
        paths.extend(std::env::split_paths(&path));
        if let Ok(joined) = std::env::join_paths(paths) {
            std::env::set_var("PATH", joined);
        }
    }
    if !Cmd::new("uv")
        .args(["python", "install", "3.13"])
        .run(ctx, "uv python install 3.13")?
    {
        ui::warn("uv python install failed");
    }
    Ok(())
}
