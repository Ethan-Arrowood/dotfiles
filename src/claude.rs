//! Install `rules` Skillfile entries without editing machine-local CLAUDE.md.
//! Claude Code skips symlinked user-scope rules in desktop Cowork sessions.

use crate::ctx::Ctx;
use crate::hooks::{self, Agent};
use crate::sys::Cmd;
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

/// Link `rules/*.md` and subdirectories except `orgs/` into `~/.claude/rules/`.
/// Subdirectories can hold rules with `paths:` frontmatter.
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

    // Sessions do not watch rules directories created after startup.
    if !ctx.dry_run {
        ui::detail("new or changed rule files load at the start of the next session");
    }
    Ok(())
}

/// Link org rules at `<src_root>/<org>/AGENTS.md` where the org directory exists.
/// User-level `paths:` patterns cannot match GitHub orgs. Parent AGENTS.md files
/// cover repos under that org. Preserve existing files that are not our links.
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
            // Remove the old import only when AGENTS.md replaces it, or rules would vanish.
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

/// Back up exact legacy org imports to avoid loading rules twice.
/// Preserve other CLAUDE.md content.
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

/// Built-in plugin controlling whether AGENTS.md loads beside CLAUDE.md.
const AGENTS_MD_PLUGIN: &str = "cc-plugin-agents-md@builtin";
const INSTRUCTION_FILES: &str = "instructionFiles";
pub const BOTH: &str = "claude-md-and-agents-md";

/// Set `pluginConfigs.<plugin>.options.<key>` only when absent.
/// Return the existing value as an inner `Err` if it differs.
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

/// Load org AGENTS.md files even when a repo has CLAUDE.md.
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

/// Whether `link_org_rules` linked any org rule under `src_root`.
pub fn has_org_rule_links(src_root: &Path) -> bool {
    let orgs = Path::new("rules").join(ORGS);
    entries(src_root).iter().any(|dir| {
        std::fs::read_link(dir.join("AGENTS.md"))
            .is_ok_and(|t| t.parent().is_some_and(|p| p.ends_with(&orgs)))
    })
}

/// Claude Code's effective `instructionFiles` value.
#[derive(Debug, PartialEq)]
pub struct InstructionFiles {
    pub value: String,
    /// A settings file sets it, possibly to an invalid value.
    pub configured: bool,
}

impl InstructionFiles {
    pub fn loads_both(&self) -> bool {
        self.value == BOTH
    }
}

/// Ask Claude Code instead of reading settings.json. It accepts the plugin's
/// old id and reports the default for an unset or invalid value.
/// `None` if `claude` can't answer.
pub fn instruction_files() -> Option<InstructionFiles> {
    let out = Cmd::new("claude")
        .args(["plugin", "configure", "--json", AGENTS_MD_PLUGIN])
        .capture()
        .ok()?;
    if !out.ok {
        return None;
    }
    parse_instruction_files(&out.stdout)
}

fn parse_instruction_files(json: &str) -> Option<InstructionFiles> {
    let v: Value = serde_json::from_str(json).ok()?;
    Some(InstructionFiles {
        value: v["inputs"][INSTRUCTION_FILES].as_str()?.to_string(),
        configured: v["configured"]
            .as_array()
            .is_some_and(|a| a.iter().any(|k| k == INSTRUCTION_FILES)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tmpdir::PrivateTempDir;
    use std::fs;

    /// Link org rules, back up legacy imports, and preserve user files.
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
        assert!(!has_org_rule_links(&dev));

        link_org_rules(&ctx, &orgs, &dev).unwrap();
        assert!(has_org_rule_links(&dev));

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

    /// Shapes from `claude plugin configure --json` (Claude Code 2.1.325).
    #[test]
    fn instruction_files_read_from_plugin_configure() {
        let parse = |inputs: &str, configured: &str| {
            parse_instruction_files(&format!(
                r#"{{"pluginId":"cc-plugin-agents-md@builtin","inputs":{{"instructionFiles":"{inputs}"}},"configured":{configured}}}"#
            ))
        };
        let both = parse(BOTH, r#"["instructionFiles"]"#).unwrap();
        assert!(both.configured && both.loads_both());
        let unset = parse("claude-md-or-agents-md", "[]").unwrap();
        assert!(!unset.configured && !unset.loads_both());
        assert_eq!(
            parse("claude-md", r#"["instructionFiles"]"#),
            Some(InstructionFiles {
                value: "claude-md".into(),
                configured: true
            })
        );
        assert_eq!(parse_instruction_files(r#"{"inputs":{}}"#), None);
        assert_eq!(parse_instruction_files("Unknown plugin"), None);
    }
}
