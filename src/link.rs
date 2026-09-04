//! Symlinking with backups. Everything a layer places in $HOME goes through
//! here so re-runs are idempotent and nothing is silently destroyed.

use crate::ctx::Ctx;
use crate::ui;
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Symlink `src` to `dst`, backing up whatever was there.
pub fn link_file(ctx: &Ctx, src: &Path, dst: &Path) -> Result<()> {
    if !src.exists() {
        ui::warn(&format!("missing source: {}", src.display()));
        return Ok(());
    }

    // Already pointing where we want it.
    if let Ok(target) = fs::read_link(dst) {
        if target == src {
            ui::skip(&format!("{} (already linked)", ctx.pretty(dst)));
            return Ok(());
        }
    }

    // symlink_metadata so a broken symlink still counts as present.
    if fs::symlink_metadata(dst).is_ok() {
        if ctx.dry_run {
            ui::dry(&format!(
                "back up {} -> {}",
                ctx.pretty(dst),
                ctx.pretty(&ctx.backup_dir)
            ));
        } else {
            fs::create_dir_all(&ctx.backup_dir)?;
            let name = dst.file_name().context("destination has no file name")?;
            fs::rename(dst, ctx.backup_dir.join(name))
                .with_context(|| format!("backing up {}", dst.display()))?;
            ui::warn(&format!(
                "backed up existing {} -> {}/",
                ctx.pretty(dst),
                ctx.pretty(&ctx.backup_dir)
            ));
        }
    }

    if ctx.dry_run {
        ui::dry(&format!("link {} -> {}", ctx.pretty(dst), ctx.pretty(src)));
        return Ok(());
    }

    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    std::os::unix::fs::symlink(src, dst)
        .with_context(|| format!("linking {} -> {}", dst.display(), src.display()))?;
    ui::ok(&format!("{} -> {}", ctx.pretty(dst), ctx.pretty(src)));
    Ok(())
}

/// Link every entry in `<layer_dir>/home` into $HOME.
pub fn link_home_dir(ctx: &Ctx, layer_dir: &Path) -> Result<()> {
    let home_src = layer_dir.join("home");
    if !home_src.is_dir() {
        return Ok(());
    }
    let mut entries: Vec<_> = fs::read_dir(&home_src)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    entries.sort();
    for entry in entries {
        let Some(name) = entry.file_name() else {
            continue;
        };
        link_file(ctx, &entry, &ctx.home.join(name))?;
    }
    Ok(())
}

/// Write a generated (non-symlinked) file.
pub fn write_file(ctx: &Ctx, dst: &Path, content: &str) -> Result<()> {
    if ctx.dry_run {
        ui::dry(&format!(
            "write {} ({} lines)",
            ctx.pretty(dst),
            content.lines().count()
        ));
        return Ok(());
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(dst, content).with_context(|| format!("writing {}", dst.display()))?;
    ui::ok(&format!("wrote {}", ctx.pretty(dst)));
    Ok(())
}

/// Remove a generated file if present.
pub fn remove_if_present(ctx: &Ctx, path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if ctx.dry_run {
        ui::dry(&format!("remove stale {}", ctx.pretty(path)));
        return Ok(());
    }
    fs::remove_file(path)?;
    ui::ok(&format!("removed stale {}", ctx.pretty(path)));
    Ok(())
}

#[cfg(unix)]
pub fn chmod(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .with_context(|| format!("chmod {mode:o} {}", path.display()))
}
