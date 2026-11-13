//! Language toolchains. Each installer is skipped when already present.

use crate::ctx::Ctx;
use crate::layer::Layer;
use crate::sys::{self, Cmd};
use crate::{brew, link, ui};
use anyhow::{Context, Result};
use std::path::Path;

pub fn install(ctx: &Ctx, layer: &Layer) -> Result<()> {
    let dir = layer.path(ctx);
    ui::step("installing toolchain packages");
    brew::bundle(ctx, &dir.join("Brewfile"))?;
    brew::load_shellenv()?;
    node(ctx)?;
    npmrc(ctx, &dir.join("npmrc"))?;
    rust(ctx)?;
    python(ctx)?;
    Ok(())
}

/// Merge `npmrc` into `~/.npmrc` key by key. npm writes the registry auth
/// token there on login, so the file is never linked or replaced: other lines
/// stay, a changed file is backed up first, and matching keys are left alone.
fn npmrc(ctx: &Ctx, src: &Path) -> Result<()> {
    let text =
        std::fs::read_to_string(src).with_context(|| format!("reading {}", src.display()))?;
    let want = ini_pairs(&text);
    let dst = ctx.home.join(".npmrc");
    // A link left by an older install points into the repo; read through it,
    // then replace it with a real file.
    let is_link = dst.is_symlink();
    let have = std::fs::read_to_string(&dst).unwrap_or_default();
    let merged = merge_ini(&have, &want);
    if merged == have && !is_link {
        ui::skip("~/.npmrc already set");
    } else if ctx.dry_run {
        ui::dry("merge npm settings into ~/.npmrc");
    } else {
        if dst.exists() {
            let backup = link::backup_path(ctx, &dst);
            if !backup.exists() {
                if let Some(parent) = backup.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(&dst, &backup)?;
            }
        }
        if is_link {
            std::fs::remove_file(&dst)?;
        }
        std::fs::write(&dst, &merged)?;
        link::chmod(&dst, 0o600)?;
        ui::ok("merged npm settings into ~/.npmrc");
    }
    let global = ctx.home.join(".npm-global");
    if !ctx.dry_run {
        std::fs::create_dir_all(&global)?;
    }
    Ok(())
}

/// `key=value` lines, skipping blanks and `#`/`;` comments.
fn ini_pairs(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with(';'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

/// Set each wanted key in an ini file's text: replace the value where the key
/// exists, append it where it doesn't, and keep every other line as is.
fn merge_ini(have: &str, want: &[(String, String)]) -> String {
    let mut lines: Vec<String> = have.lines().map(String::from).collect();
    for (k, v) in want {
        let line = format!("{k}={v}");
        match lines
            .iter()
            .position(|l| l.split_once('=').is_some_and(|(lk, _)| lk.trim() == k))
        {
            Some(i) => {
                if lines[i].split_once('=').map(|(_, lv)| lv.trim()) != Some(v.as_str()) {
                    lines[i] = line;
                }
            }
            None => lines.push(line),
        }
    }
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
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
    // Prevent rustup from appending through ~/.zshenv into tracked repo content.
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
        // Network and version churn on every run otherwise; rustup above
        // follows the same rule.
        if ctx.update {
            let _ = Cmd::new("uv")
                .args(["self", "update"])
                .run(ctx, "uv self update");
        }
        return python_313(ctx);
    }
    if ctx.dry_run {
        ui::dry("install uv");
        return Ok(());
    }
    ui::step("installing uv");
    // Prevent uv from editing the symlinked ~/.zshenv, which once changed this repo.
    // Support cargo-dist's INSTALLER_NO_MODIFY_PATH and uv's UV_NO_MODIFY_PATH.
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
    python_313(ctx)
}

/// Checked on its own, so a run after a failed Python install retries it.
fn python_313(ctx: &Ctx) -> Result<()> {
    if !ctx.dry_run
        && Cmd::new("uv")
            .args(["python", "find", "3.13"])
            .capture()
            .is_ok_and(|c| c.ok)
    {
        ui::skip("python 3.13 already installed");
        return Ok(());
    }
    if !Cmd::new("uv")
        .args(["python", "install", "3.13"])
        .run(ctx, "uv python install 3.13")?
    {
        ui::warn("uv python install failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The auth token npm wrote stays; wanted keys are set or replaced.
    #[test]
    fn merge_ini_keeps_other_lines() {
        let have = "//registry.npmjs.org/:_authToken=secret\nfund=true\n";
        let want = ini_pairs("# note\nfund=false\nprefix=~/.npm-global\n");
        let out = merge_ini(have, &want);
        assert_eq!(
            out,
            "//registry.npmjs.org/:_authToken=secret\nfund=false\nprefix=~/.npm-global\n"
        );
        // A second merge changes nothing.
        assert_eq!(merge_ini(&out, &want), out);
    }

    #[test]
    fn merge_ini_writes_a_missing_file() {
        let want = ini_pairs("fund=false\n");
        assert_eq!(merge_ini("", &want), "fund=false\n");
    }
}
