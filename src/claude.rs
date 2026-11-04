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

/// Org rules are linked into each org directory, not `~/.claude/rules/`; see
/// `link_org_rules`.
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

/// `rules/orgs/<org>.md` -> a symlink at `<src_root>/<org>/AGENTS.md`.
///
/// `paths:` frontmatter cannot match by GitHub org, because user-level
/// patterns are relative to the session's working directory. Claude Code
/// does load every AGENTS.md above the session, and repos live at
/// `~/dev/<org>/<repo>`, so an AGENTS.md in the org directory covers them all.
///
/// Only where the org directory exists, as agentic's own sync hook does. An
/// existing AGENTS.md that is not ours is left alone.
fn link_org_rules(ctx: &Ctx, orgs_dir: &Path, src_root: &Path) -> Result<()> {
    let rules: Vec<PathBuf> = entries(orgs_dir).into_iter().filter(|p| is_md(p)).collect();
    if rules.is_empty() {
        return Ok(());
    }
    let mut linked = false;
    for rule in rules {
        let dir = src_root.join(rule.file_stem().unwrap());
        if !dir.is_dir() {
            continue;
        }
        let dst = dir.join("AGENTS.md");
        let ours = std::fs::read_link(&dst).is_ok_and(|t| t == rule);
        let dangling = dst.is_symlink() && !dst.exists();
        if ours || dangling || !dst.exists() {
            // Only once the AGENTS.md link is (or will be) ours: removing the
            // old import otherwise leaves the org with no rules at all.
            remove_old_import(ctx, &rule, &dir.join("CLAUDE.md"))?;
            link::link_file(ctx, &rule, &dst)?;
            linked = true;
        } else {
            ui::warn(&format!(
                "{} exists and is not a link to {}; leaving it",
                ctx.pretty(&dst),
                ctx.pretty(&rule)
            ));
        }
    }
    if linked {
        load_agents_md_beside_claude_md(ctx)?;
    }
    Ok(())
}

/// Older versions of this tool and of agentic's hook imported org rules from
/// `<org>/CLAUDE.md`. Next to the AGENTS.md link that file would load the
/// rules twice, so a CLAUDE.md that is exactly that one import line moves to
/// the backup directory. Any other CLAUDE.md is the user's and stays.
fn remove_old_import(ctx: &Ctx, rule: &Path, claude_md: &Path) -> Result<()> {
    if claude_md.is_symlink() {
        return Ok(());
    }
    let Ok(text) = std::fs::read_to_string(claude_md) else {
        return Ok(());
    };
    let text = text.trim();
    if text != format!("@{}", ctx.pretty(rule)) && text != format!("@{}", rule.display()) {
        return Ok(());
    }
    if ctx.dry_run {
        ui::dry(&format!(
            "remove {} (replaced by AGENTS.md)",
            ctx.pretty(claude_md)
        ));
        return Ok(());
    }
    let backup = link::backup_path(ctx, claude_md);
    if backup.exists() {
        ui::warn(&format!(
            "backup {} exists; leaving {}",
            backup.display(),
            ctx.pretty(claude_md)
        ));
        return Ok(());
    }
    if let Some(parent) = backup.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(claude_md, &backup)
        .with_context(|| format!("moving {} to the backup", claude_md.display()))?;
    ui::ok(&format!(
        "removed {} (replaced by AGENTS.md)",
        ctx.pretty(claude_md)
    ));
    Ok(())
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

/// By default any CLAUDE.md in or above a repo makes Claude Code skip every
/// AGENTS.md, the org rules included. Several repos have their own CLAUDE.md,
/// so once org rules are linked, load both.
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
                    "{INSTRUCTION_FILES} is set to {other}; leaving it. Repos with a CLAUDE.md will skip the org AGENTS.md unless it is {BOTH}"
                ));
                Ok(false)
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tmpdir::PrivateTempDir;
    use std::fs;

    /// Org rules link as `<org>/AGENTS.md` only where the org directory
    /// exists, an old one-line CLAUDE.md import in either form is moved out,
    /// and files that are not ours stay.
    #[test]
    fn org_rules_link_as_agents_md_and_replace_old_imports() {
        let dir = PrivateTempDir::new("orgtest").unwrap();
        let home = dir.join("home");
        let mut ctx = Ctx::new(dir.join("repo")).unwrap();
        ctx.home = home.clone();
        ctx.backup_dir = home.join(".dotfiles-backup/test");
        let dev = home.join("dev");
        let orgs = dev.join("me/agentic/rules/orgs");
        fs::create_dir_all(&orgs).unwrap();
        for org in ["abs", "tilde", "mine", "theirs", "absent"] {
            fs::write(orgs.join(format!("{org}.md")), "rule").unwrap();
        }
        for org in ["abs", "tilde", "mine", "theirs"] {
            fs::create_dir_all(dev.join(org)).unwrap();
        }
        let rule = |org: &str| orgs.join(format!("{org}.md"));
        fs::write(
            dev.join("abs/CLAUDE.md"),
            format!("@{}\n", rule("abs").display()),
        )
        .unwrap();
        fs::write(
            dev.join("tilde/CLAUDE.md"),
            "@~/dev/me/agentic/rules/orgs/tilde.md\n",
        )
        .unwrap();
        fs::write(dev.join("mine/CLAUDE.md"), "my own notes\n").unwrap();
        fs::write(dev.join("theirs/AGENTS.md"), "hand-written").unwrap();

        link_org_rules(&ctx, &orgs, &dev).unwrap();

        for org in ["abs", "tilde", "mine"] {
            assert_eq!(
                fs::read_link(dev.join(org).join("AGENTS.md")).unwrap(),
                rule(org)
            );
        }
        assert!(!dev.join("abs/CLAUDE.md").exists());
        assert!(!dev.join("tilde/CLAUDE.md").exists());
        assert!(ctx.backup_dir.join("dev/abs/CLAUDE.md").is_file());
        assert_eq!(
            fs::read_to_string(dev.join("mine/CLAUDE.md")).unwrap(),
            "my own notes\n"
        );
        assert_eq!(
            fs::read_to_string(dev.join("theirs/AGENTS.md")).unwrap(),
            "hand-written"
        );
        assert!(!dev.join("absent").exists());

        // A second run changes nothing.
        link_org_rules(&ctx, &orgs, &dev).unwrap();
        assert_eq!(
            fs::read_link(dev.join("abs/AGENTS.md")).unwrap(),
            rule("abs")
        );
    }

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
