//! Symlinking with backups. Everything a layer places in $HOME goes through
//! here so re-runs are idempotent and nothing is silently destroyed.

use crate::ctx::Ctx;
use crate::ui;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Where a displaced file goes, preserving its location under the backup root.
///
/// Keyed on the basename alone, `~/.zshrc`, `~/.claude/rules/general.md` and
/// `~/.claude/skills/general` all landed on top of each other -- and
/// `fs::rename` silently replaces an existing file, so the earlier backup was
/// destroyed without a word.
fn backup_path(ctx: &Ctx, dst: &Path) -> PathBuf {
    match dst.strip_prefix(&ctx.home) {
        Ok(rel) => ctx.backup_dir.join(rel),
        Err(_) => ctx.backup_dir.join(dst.strip_prefix("/").unwrap_or(dst)),
    }
}

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
    if let Ok(meta) = fs::symlink_metadata(dst) {
        // A real directory is almost certainly the user's own content -- a
        // hand-written skill, a config tree -- and moving it wholesale into a
        // backup is too destructive to do silently.
        if meta.is_dir() && !meta.file_type().is_symlink() {
            ui::warn(&format!(
                "{} is a real directory, not a link; leaving it alone",
                ctx.pretty(dst)
            ));
            ui::detail(&format!(
                "move or remove it if you want it replaced by\n  {}",
                ctx.pretty(src)
            ));
            return Ok(());
        }

        let backup = backup_path(ctx, dst);
        if ctx.dry_run {
            ui::dry(&format!(
                "back up {} -> {}",
                ctx.pretty(dst),
                ctx.pretty(&backup)
            ));
        } else {
            if let Some(parent) = backup.parent() {
                fs::create_dir_all(parent)?;
            }
            // Never write over an existing backup: rename replaces silently.
            if fs::symlink_metadata(&backup).is_ok() {
                anyhow::bail!(
                    "refusing to overwrite an existing backup at {}",
                    backup.display()
                );
            }
            fs::rename(dst, &backup).with_context(|| format!("backing up {}", dst.display()))?;
            ui::warn(&format!(
                "backed up existing {} -> {}",
                ctx.pretty(dst),
                ctx.pretty(&backup)
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
    // symlink_metadata, not exists(): exists() follows the link and is false
    // for a dangling one, so a broken symlink was never cleaned up.
    if fs::symlink_metadata(path).is_err() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tmpdir::PrivateTempDir;

    /// A Ctx rooted in a throwaway home, so nothing touches the real one.
    fn scratch() -> (PrivateTempDir, Ctx) {
        let dir = PrivateTempDir::new("linktest").unwrap();
        let home = dir.join("home");
        fs::create_dir_all(&home).unwrap();
        let mut ctx = Ctx::new(dir.join("repo"), false, true).unwrap();
        ctx.home = home.clone();
        ctx.backup_dir = home.join(".dotfiles-backup/test");
        ctx.state_dir = home.join(".config/dotfiles");
        (dir, ctx)
    }

    fn src_file(ctx: &Ctx, name: &str) -> PathBuf {
        let p = ctx.root.join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, b"managed").unwrap();
        p
    }

    #[test]
    fn links_and_is_idempotent() {
        let (_d, ctx) = scratch();
        let src = src_file(&ctx, "a.txt");
        let dst = ctx.home.join(".a");
        link_file(&ctx, &src, &dst).unwrap();
        assert_eq!(fs::read_link(&dst).unwrap(), src);

        // Second call must not disturb it or produce a backup.
        link_file(&ctx, &src, &dst).unwrap();
        assert_eq!(fs::read_link(&dst).unwrap(), src);
        assert!(
            !ctx.backup_dir.exists(),
            "a no-op run must not create a backup"
        );
    }

    #[test]
    fn existing_file_is_backed_up_with_its_content_intact() {
        let (_d, ctx) = scratch();
        let src = src_file(&ctx, "b.txt");
        let dst = ctx.home.join(".b");
        fs::write(&dst, b"the user's original").unwrap();

        link_file(&ctx, &src, &dst).unwrap();
        assert_eq!(fs::read_link(&dst).unwrap(), src);
        let backed = ctx.backup_dir.join(".b");
        assert_eq!(fs::read(backed).unwrap(), b"the user's original");
    }

    /// Two destinations sharing a basename used to collide in the backup dir,
    /// and `rename` replaces silently -- so the first file was destroyed.
    #[test]
    fn same_basename_in_two_places_does_not_clobber_the_first_backup() {
        let (_d, ctx) = scratch();
        let src = src_file(&ctx, "config");

        let one = ctx.home.join(".ssh/config");
        let two = ctx.home.join(".config/ghostty/config");
        for (p, content) in [
            (&one, &b"ssh original"[..]),
            (&two, &b"ghostty original"[..]),
        ] {
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, content).unwrap();
        }

        link_file(&ctx, &src, &one).unwrap();
        link_file(&ctx, &src, &two).unwrap();

        assert_eq!(
            fs::read(ctx.backup_dir.join(".ssh/config")).unwrap(),
            b"ssh original"
        );
        assert_eq!(
            fs::read(ctx.backup_dir.join(".config/ghostty/config")).unwrap(),
            b"ghostty original"
        );
    }

    /// A hand-written skill directory must survive a Skillfile that names it.
    #[test]
    fn a_real_directory_is_left_alone() {
        let (_d, ctx) = scratch();
        let src = src_file(&ctx, "skill-src");
        let dst = ctx.home.join(".claude/skills/learned");
        fs::create_dir_all(&dst).unwrap();
        fs::write(dst.join("SKILL.md"), b"hand written").unwrap();

        link_file(&ctx, &src, &dst).unwrap();

        assert!(dst.is_dir(), "real directory must not be replaced");
        assert_eq!(fs::read(dst.join("SKILL.md")).unwrap(), b"hand written");
        assert!(!ctx.backup_dir.exists(), "and must not be moved to backup");
    }

    #[test]
    fn a_broken_symlink_destination_is_replaced() {
        let (_d, ctx) = scratch();
        let src = src_file(&ctx, "c.txt");
        let dst = ctx.home.join(".c");
        std::os::unix::fs::symlink(ctx.home.join("nowhere"), &dst).unwrap();

        link_file(&ctx, &src, &dst).unwrap();
        assert_eq!(fs::read_link(&dst).unwrap(), src);
    }

    #[test]
    fn refuses_to_overwrite_an_existing_backup() {
        let (_d, ctx) = scratch();
        let src = src_file(&ctx, "d.txt");
        let dst = ctx.home.join(".d");
        fs::write(&dst, b"first").unwrap();
        link_file(&ctx, &src, &dst).unwrap();

        // Re-create the destination and force a second backup of the same path.
        fs::remove_file(&dst).unwrap();
        fs::write(&dst, b"second").unwrap();
        let err = link_file(&ctx, &src, &dst).unwrap_err();
        assert!(
            err.to_string().contains("existing backup"),
            "should refuse rather than destroy the first backup: {err}"
        );
        assert_eq!(fs::read(ctx.backup_dir.join(".d")).unwrap(), b"first");
    }

    #[test]
    fn remove_if_present_clears_a_dangling_symlink() {
        let (_d, ctx) = scratch();
        let p = ctx.home.join("dangling");
        std::os::unix::fs::symlink(ctx.home.join("missing"), &p).unwrap();
        assert!(
            !p.exists(),
            "precondition: exists() is false for a dangling link"
        );

        remove_if_present(&ctx, &p).unwrap();
        assert!(
            fs::symlink_metadata(&p).is_err(),
            "dangling link should be gone"
        );
    }
}
