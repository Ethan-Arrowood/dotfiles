//! Claude Code configuration contributed by layers.
//!
//! Rule files, not CLAUDE.md. `~/.claude/rules/*.md` all load every session,
//! so the repo can own whole topic files while `~/.claude/CLAUDE.md` stays
//! untracked and machine-specific. That avoids merging markdown sections into
//! a file the user also edits by hand.
//!
//! Note: Claude Code skips a symlinked user-scope rule file in Cowork sessions
//! on the desktop. Everywhere else it follows the link normally.

use crate::ctx::Ctx;
use crate::layer::Layer;
use crate::{link, ui};
use anyhow::Result;

/// Link `<layer>/claude/rules/*.md` into `~/.claude/rules/`.
pub fn link_rules(ctx: &Ctx, layer: &Layer) -> Result<()> {
    let src_dir = layer.path(ctx).join("claude/rules");
    if !src_dir.is_dir() {
        return Ok(());
    }
    let dest_dir = ctx.home.join(".claude/rules");
    if !ctx.dry_run {
        std::fs::create_dir_all(&dest_dir)?;
    }

    let mut files: Vec<_> = std::fs::read_dir(&src_dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().map(|x| x == "md").unwrap_or(false))
        .collect();
    files.sort();
    if files.is_empty() {
        return Ok(());
    }

    for f in &files {
        let name = f.file_name().unwrap().to_owned();
        link::link_file(ctx, f, &dest_dir.join(name))?;
    }

    // A rules directory that did not exist when the session started is not
    // watched, so the current session will not see it.
    if !ctx.dry_run {
        ui::detail("new or changed rule files load at the start of the next session");
    }
    Ok(())
}
