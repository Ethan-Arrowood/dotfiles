//! Claude Code rules, from `rules` Skillfile entries.
//!
//! Rule files, not CLAUDE.md. `~/.claude/rules/*.md` all load every session,
//! so the repo can own whole topic files while `~/.claude/CLAUDE.md` stays
//! untracked and machine-specific. That avoids merging markdown sections into
//! a file the user also edits by hand.
//!
//! Note: Claude Code skips a symlinked user-scope rule file in Cowork sessions
//! on the desktop. Everywhere else it follows the link normally.

use crate::ctx::Ctx;
use crate::hooks::{self, Agent};
use crate::{link, ui};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Org rules are imported per directory instead of linked; see `link_org_rules`.
const ORGS: &str = "orgs";

fn entries(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn is_md(p: &Path) -> bool {
    p.is_file() && p.extension().is_some_and(|x| x == "md")
}

/// Link a repo's `rules/` into `~/.claude/rules/`: every `*.md`, and every
/// subdirectory except `orgs/`. Subdirectories such as `lang/` hold rules
/// with `paths:` frontmatter, so they load only for matching files.
pub fn link_rules(ctx: &Ctx, src_dir: &Path, src_root: &Path) -> Result<()> {
    if !src_dir.is_dir() {
        ui::warn(&format!("no rules directory at {}", ctx.pretty(src_dir)));
        return Ok(());
    }
    let dest_dir = ctx.home.join(".claude/rules");
    for p in entries(src_dir) {
        let name = p.file_name().unwrap();
        if is_md(&p) || (p.is_dir() && name != ORGS) {
            link::link_file(ctx, &p, &dest_dir.join(name))?;
        }
    }
    link_org_rules(ctx, &src_dir.join(ORGS), src_root)?;

    // A rules directory that did not exist when the session started is not
    // watched, so the current session will not see it.
    if !ctx.dry_run {
        ui::detail("new or changed rule files load at the start of the next session");
    }
    Ok(())
}

/// `rules/orgs/<org>.md` -> an import line in `<src_root>/<org>/CLAUDE.md`.
///
/// `paths:` frontmatter cannot match by GitHub org, because user-level
/// patterns are relative to the session's working directory. Claude Code
/// does load every CLAUDE.md above the session, and repos live at
/// `~/dev/<org>/<repo>`, so a CLAUDE.md in the org directory covers them all.
///
/// An existing CLAUDE.md without the import is left alone: it may hold the
/// user's own lines, and is not ours to rewrite.
fn link_org_rules(ctx: &Ctx, orgs_dir: &Path, src_root: &Path) -> Result<()> {
    let rules: Vec<PathBuf> = entries(orgs_dir).into_iter().filter(|p| is_md(p)).collect();
    if rules.is_empty() {
        return Ok(());
    }
    for rule in rules {
        let org = rule.file_stem().unwrap();
        let dst = src_root.join(org).join("CLAUDE.md");
        let line = format!("@{}", ctx.pretty(&rule));
        // agentic's own sync hook writes the absolute form.
        let absolute = format!("@{}", rule.display());
        match std::fs::read_to_string(&dst) {
            Ok(text)
                if text
                    .lines()
                    .any(|l| l.trim() == line || l.trim() == absolute) =>
            {
                ui::skip(&format!("{} (already imports {})", ctx.pretty(&dst), line));
            }
            Ok(_) => {
                ui::warn(&format!(
                    "{} exists without the org rules; add this line to it:",
                    ctx.pretty(&dst)
                ));
                ui::detail(&line);
            }
            Err(_) => link::write_file(ctx, &dst, &format!("{line}\n"))?,
        }
    }
    load_agents_md_beside_claude_md(ctx)
}

/// The built-in plugin that loads AGENTS.md, and the option that decides
/// whether it loads beside CLAUDE.md or only in its absence.
const AGENTS_MD_PLUGIN: &str = "cc-plugin-agents-md@builtin";
const INSTRUCTION_FILES: &str = "instructionFiles";
const BOTH: &str = "claude-md-and-agents-md";

/// Set `pluginConfigs.<plugin>.options.<key>` unless it is already set.
/// Returns `Err` with the existing value when it is set to something else,
/// so a deliberate choice is never overridden.
fn set_plugin_option(
    settings: &mut Value,
    plugin: &str,
    key: &str,
    value: &str,
) -> Result<std::result::Result<bool, String>> {
    let root = settings
        .as_object_mut()
        .context("settings.json is not a JSON object")?;
    let options = root
        .entry("pluginConfigs")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("`pluginConfigs` in settings.json is not an object")?
        .entry(plugin)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .with_context(|| format!("`pluginConfigs.{plugin}` is not an object"))?
        .entry("options")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .with_context(|| format!("`pluginConfigs.{plugin}.options` is not an object"))?;
    match options.get(key) {
        Some(v) if v == value => Ok(Ok(false)),
        Some(v) => Ok(Err(v.to_string())),
        None => {
            options.insert(key.into(), json!(value));
            Ok(Ok(true))
        }
    }
}

/// A CLAUDE.md above a repo stops Claude Code reading that repo's own
/// AGENTS.md (nodejs/node has one), so once org imports exist, load both.
fn load_agents_md_beside_claude_md(ctx: &Ctx) -> Result<()> {
    let what = format!("set {INSTRUCTION_FILES} to {BOTH}");
    hooks::edit_json(
        ctx,
        &Agent::Claude.hooks_file(ctx),
        &what,
        |s| match set_plugin_option(s, AGENTS_MD_PLUGIN, INSTRUCTION_FILES, BOTH)? {
            Ok(true) => Ok(true),
            Ok(false) => {
                ui::skip(&format!("{INSTRUCTION_FILES} is already {BOTH}"));
                Ok(false)
            }
            Err(other) => {
                ui::warn(&format!(
                    "{INSTRUCTION_FILES} is set to {other}; leaving it. Org CLAUDE.md files will hide repos' AGENTS.md unless it is {BOTH}"
                ));
                Ok(false)
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_option_is_set_once_and_never_overridden() {
        let mut s = json!({ "model": "x" });
        let set = |s: &mut Value| set_plugin_option(s, "p@builtin", "k", "both").unwrap();
        assert_eq!(set(&mut s), Ok(true));
        assert_eq!(s["pluginConfigs"]["p@builtin"]["options"]["k"], "both");
        assert_eq!(set(&mut s), Ok(false));
        s["pluginConfigs"]["p@builtin"]["options"]["k"] = json!("mine");
        assert_eq!(set(&mut s), Err("\"mine\"".into()));
        assert_eq!(s["pluginConfigs"]["p@builtin"]["options"]["k"], "mine");
    }
}
