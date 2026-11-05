//! Merge `hook` Skillfile entries into agent JSON files shared with the user.
//! Both agents use `hooks.<Event>[{ matcher?, hooks: [{ type, command, timeout? }] }]`.

use crate::ctx::Ctx;
use crate::sys::Cmd;
use crate::{link, shellinit, ui};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    Claude,
    Codex,
}

impl Agent {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "claude" => Some(Agent::Claude),
            "codex" => Some(Agent::Codex),
            _ => None,
        }
    }

    fn config_dir(self, ctx: &Ctx) -> PathBuf {
        match self {
            Agent::Claude => ctx.home.join(".claude"),
            Agent::Codex => ctx.home.join(".codex"),
        }
    }

    /// The JSON file the agent reads hooks from.
    pub fn hooks_file(self, ctx: &Ctx) -> PathBuf {
        match self {
            Agent::Claude => self.config_dir(ctx).join("settings.json"),
            Agent::Codex => self.config_dir(ctx).join("hooks.json"),
        }
    }

    /// Set the agent config directory for install-time hooks to match this tool.
    fn config_env(self) -> &'static str {
        match self {
            Agent::Claude => "CLAUDE_CONFIG_DIR",
            Agent::Codex => "CODEX_HOME",
        }
    }

    /// Directories a sync hook installs into only if they already exist.
    fn install_dirs(self, ctx: &Ctx) -> Vec<PathBuf> {
        match self {
            Agent::Claude => vec![
                ctx.home.join(".claude/rules"),
                ctx.home.join(".claude/skills"),
            ],
            Agent::Codex => vec![ctx.home.join(".codex"), ctx.home.join(".agents/skills")],
        }
    }

    /// Codex's SessionStart hook runs on new sessions only, as agentic's
    /// Codex README registers it.
    fn matcher(self, event: &str) -> Option<&'static str> {
        match (self, event) {
            (Agent::Codex, "SessionStart") => Some("startup"),
            _ => None,
        }
    }
}

/// Add `command` unless it or an alias exists. Return whether it changed.
/// Preserve other entries and their key order.
fn add_hook(
    settings: &mut Value,
    event: &str,
    matcher: Option<&str>,
    command: &str,
    aliases: &[&str],
    timeout: Option<u32>,
) -> Result<bool> {
    let root = settings
        .as_object_mut()
        .context("the hooks file is not a JSON object")?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("`hooks` is not an object")?;
    let groups = hooks
        .entry(event)
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .with_context(|| format!("`hooks.{event}` is not an array"))?;

    let present = groups.iter().any(|g| {
        g["hooks"].as_array().is_some_and(|hs| {
            hs.iter()
                .any(|h| h["command"] == command || aliases.iter().any(|a| h["command"] == *a))
        })
    });
    if present {
        return Ok(false);
    }
    let mut hook = json!({ "type": "command", "command": command });
    if let Some(t) = timeout {
        hook["timeout"] = json!(t);
    }
    let mut group = json!({});
    if let Some(m) = matcher {
        group["matcher"] = json!(m);
    }
    group["hooks"] = json!([hook]);
    groups.push(group);
    Ok(true)
}

/// Apply `edit`, back up and write only if changed. Leave unparseable files alone.
pub(crate) fn edit_json(
    ctx: &Ctx,
    path: &Path,
    what: &str,
    edit: impl FnOnce(&mut Value) -> Result<bool>,
) -> Result<()> {
    let mut settings: Value = match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                ui::warn(&format!(
                    "could not parse {} ({e}); not changing it to {what}",
                    ctx.pretty(path)
                ));
                return Ok(());
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };

    // Warn on invalid shapes without changing the file or aborting installation.
    match edit(&mut settings) {
        Ok(true) => {}
        Ok(false) => return Ok(()),
        Err(e) => {
            ui::warn(&format!(
                "{}: {e:#}; not changing it to {what}",
                ctx.pretty(path)
            ));
            return Ok(());
        }
    }
    if ctx.dry_run {
        ui::dry(&format!("{what} in {}", ctx.pretty(path)));
        return Ok(());
    }
    // Back up once per run so later edits cannot replace the original backup.
    let backup = link::backup_path(ctx, path);
    if path.exists() && !backup.exists() {
        if let Some(parent) = backup.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(path, &backup).with_context(|| format!("backing up {}", path.display()))?;
    }
    let mut out = serde_json::to_string_pretty(&settings)?;
    out.push('\n');
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Rename a sibling over the target so agents never read partial JSON.
    // Preserve permissions and write through symlinks.
    let target = if path.is_symlink() {
        std::fs::canonicalize(path).with_context(|| format!("resolving {}", path.display()))?
    } else {
        path.to_path_buf()
    };
    let tmp = target.with_extension(format!("json.dotfiles-{}", std::process::id()));
    std::fs::write(&tmp, out).with_context(|| format!("writing {}", tmp.display()))?;
    if let Ok(meta) = std::fs::metadata(&target) {
        std::fs::set_permissions(&tmp, meta.permissions())?;
    }
    std::fs::rename(&tmp, &target).with_context(|| format!("replacing {}", target.display()))?;
    ui::ok(&format!("{what} in {}", ctx.pretty(path)));
    Ok(())
}

/// Register the hook. Run SessionStart once now so the first session has rules,
/// even before the user trusts a new Codex hook.
pub fn register(
    ctx: &Ctx,
    agent: Agent,
    script: &Path,
    event: &str,
    timeout: Option<u32>,
) -> Result<()> {
    let script_str = script.to_string_lossy();
    let command = if script_str
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-~".contains(c))
    {
        script_str.into_owned()
    } else {
        shellinit::sh_quote(&script_str)
    };
    // A hook registered by hand, as agentic's READMEs show, uses `~/`.
    let tilde = ctx.pretty(script);
    let what = format!("register {event} hook {tilde}");
    let mut added = false;
    edit_json(ctx, &agent.hooks_file(ctx), &what, |s| {
        added = add_hook(
            s,
            event,
            agent.matcher(event),
            &command,
            &[tilde.as_str()],
            timeout,
        )?;
        if !added {
            ui::skip(&format!(
                "{event} hook {} (already registered)",
                ctx.pretty(script)
            ));
        }
        Ok(added)
    })?;
    if added && agent == Agent::Codex && !ctx.dry_run {
        ui::detail("trust the new hook with /hooks in Codex");
    }

    if event == "SessionStart" {
        run_once(ctx, agent, script)?;
    }
    Ok(())
}

fn run_once(ctx: &Ctx, agent: Agent, script: &Path) -> Result<()> {
    if ctx.dry_run {
        ui::dry(&format!("run {}", ctx.pretty(script)));
        return Ok(());
    }
    for d in agent.install_dirs(ctx) {
        std::fs::create_dir_all(&d).with_context(|| format!("creating {}", d.display()))?;
    }
    // Match hook org directories to Skillfile checkout locations.
    let out = Cmd::new(script.to_string_lossy())
        .env(agent.config_env(), agent.config_dir(ctx).to_string_lossy())
        .env(
            "AGENTIC_DEV_DIR",
            crate::skills::src_root(ctx).to_string_lossy(),
        )
        .capture()?;
    for line in out.combined().lines().filter(|l| !l.trim().is_empty()) {
        ui::detail(line);
    }
    if !out.ok {
        ui::warn(&format!("{} failed", ctx.pretty(script)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_hook_keeps_existing_hooks_and_key_order() {
        let mut s: Value = serde_json::from_str(
            r#"{"model":"x","hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"other"}]}]},"permissions":{}}"#,
        )
        .unwrap();
        assert!(add_hook(&mut s, "SessionStart", None, "/a/sync.sh", &[], Some(15)).unwrap());
        let groups = s["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0]["hooks"][0]["command"], "other");
        assert_eq!(groups[1]["hooks"][0]["command"], "/a/sync.sh");
        assert_eq!(groups[1]["hooks"][0]["timeout"], 15);
        assert!(groups[1].get("matcher").is_none());
        let keys: Vec<&String> = s.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["model", "hooks", "permissions"]);
    }

    #[test]
    fn add_hook_is_idempotent_and_creates_missing_sections() {
        let mut s = json!({});
        assert!(add_hook(&mut s, "SessionStart", None, "/a/sync.sh", &[], None).unwrap());
        assert!(!add_hook(&mut s, "SessionStart", None, "/a/sync.sh", &[], None).unwrap());
        assert_eq!(s["hooks"]["SessionStart"].as_array().unwrap().len(), 1);
        assert!(s["hooks"]["SessionStart"][0]["hooks"][0]
            .get("timeout")
            .is_none());
    }

    /// A hook added by hand as `~/...` is the same hook as `/Users/me/...`.
    #[test]
    fn add_hook_matches_an_alias() {
        let mut s = json!({"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"~/a/sync.sh"}]}]}});
        assert!(!add_hook(
            &mut s,
            "SessionStart",
            None,
            "/home/me/a/sync.sh",
            &["~/a/sync.sh"],
            None
        )
        .unwrap());
        assert_eq!(s["hooks"]["SessionStart"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn codex_session_start_hooks_match_startup_only() {
        let mut s = json!({});
        let m = Agent::Codex.matcher("SessionStart");
        assert!(add_hook(&mut s, "SessionStart", m, "/a/sync.sh", &[], None).unwrap());
        let group = &s["hooks"]["SessionStart"][0];
        let keys: Vec<&String> = group.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["matcher", "hooks"]);
        assert_eq!(group["matcher"], "startup");
        assert_eq!(Agent::Claude.matcher("SessionStart"), None);
    }

    #[test]
    fn add_hook_refuses_a_malformed_shape() {
        let mut s = json!({ "hooks": [] });
        assert!(add_hook(&mut s, "SessionStart", None, "/a", &[], None).is_err());
    }
}
