//! Install skills, rules and hooks from layer Skillfiles. See README.md for syntax.
//! Clone under $DOTFILES_SRC_DIR (default ~/dev), then link the editable checkouts.

use crate::ctx::Ctx;
use crate::hooks::{self, Agent};
use crate::layer::Layer;
use crate::sys::{self, Cmd};
use crate::{claude, link, ui};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub enum Entry {
    /// Every skill in the repo.
    Repo { owner: String, name: String },
    /// One named skill from the repo.
    Skill {
        owner: String,
        name: String,
        skill: String,
    },
    /// Explicit rule opt-in prevents a skills repo from silently adding global rules.
    Rules { owner: String, name: String },
    /// A script in the repo, registered as an agent hook.
    Hook {
        agent: Agent,
        owner: String,
        name: String,
        path: String,
        event: String,
        timeout: Option<u32>,
    },
}

impl Entry {
    pub fn owner(&self) -> &str {
        match self {
            Entry::Repo { owner, .. }
            | Entry::Skill { owner, .. }
            | Entry::Rules { owner, .. }
            | Entry::Hook { owner, .. } => owner,
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Entry::Repo { name, .. }
            | Entry::Skill { name, .. }
            | Entry::Rules { name, .. }
            | Entry::Hook { name, .. } => name,
        }
    }
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner(), self.name())
    }
}

/// Strip `#` comments only at line start or after whitespace.
/// Keep the separator in `skill a/b#c`.
pub(crate) fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    for (i, &c) in bytes.iter().enumerate() {
        if c == b'#' && (i == 0 || bytes[i - 1].is_ascii_whitespace()) {
            return &line[..i];
        }
    }
    line
}

/// Parse entries and report malformed lines with line numbers.
pub fn parse(text: &str) -> (Vec<Entry>, Vec<String>) {
    let mut entries = Vec::new();
    let mut problems = Vec::new();

    for (i, raw) in text.lines().enumerate() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        let n = i + 1;
        let mut parts = line.split_whitespace();
        let kind = parts.next();
        // `hook` names the agent before the repo.
        let agent = if kind == Some("hook") {
            match parts.next().map(|a| (a, Agent::parse(a))) {
                Some((_, Some(a))) => Some(a),
                Some((a, None)) => {
                    problems.push(format!(
                        "line {n}: unknown agent `{a}`; expected `claude` or `codex`"
                    ));
                    continue;
                }
                None => None,
            }
        } else {
            None
        };
        let (Some(kind), Some(target)) = (kind, parts.next()) else {
            problems.push(format!(
                "line {n}: expected `<kind> <owner>/<repo>`, got `{line}`"
            ));
            continue;
        };

        let (slug, skill) = match target.split_once('#') {
            Some((s, k)) => (s, Some(k)),
            None => (target, None),
        };
        let Some((owner, name)) = slug.split_once('/') else {
            problems.push(format!("line {n}: `{slug}` is not <owner>/<repo>"));
            continue;
        };
        if owner.is_empty() || name.is_empty() || name.contains('/') {
            problems.push(format!("line {n}: `{slug}` is not <owner>/<repo>"));
            continue;
        }
        // Only `hook` takes more words (the event and timeout).
        if kind != "hook" {
            if let Some(extra) = parts.next() {
                problems.push(format!("line {n}: unexpected `{extra}` after `{target}`"));
                continue;
            }
        }

        match (kind, skill) {
            ("repo", None) => entries.push(Entry::Repo {
                owner: owner.to_string(),
                name: name.to_string(),
            }),
            ("repo", Some(_)) => problems.push(format!(
                "line {n}: `repo` takes no #skill suffix. Use `skill {target}`"
            )),
            ("skill", Some(k)) if !k.is_empty() => entries.push(Entry::Skill {
                owner: owner.to_string(),
                name: name.to_string(),
                skill: k.to_string(),
            }),
            ("rules", None) => entries.push(Entry::Rules {
                owner: owner.to_string(),
                name: name.to_string(),
            }),
            ("rules", Some(_)) => problems.push(format!(
                "line {n}: `rules` takes no #suffix. Use `rules {slug}`"
            )),
            ("hook", Some(path)) if !path.is_empty() => {
                let agent = agent.expect("hook lines name an agent before the target");
                let rest: Vec<&str> = parts.by_ref().collect();
                let timeout = match rest.get(1).map(|t| t.parse::<u32>()) {
                    None => None,
                    Some(Ok(t)) => Some(t),
                    Some(Err(_)) => {
                        problems.push(format!("line {n}: timeout `{}` is not a number", rest[1]));
                        continue;
                    }
                };
                match rest.first() {
                    Some(event) if rest.len() <= 2 => entries.push(Entry::Hook {
                        agent,
                        owner: owner.to_string(),
                        name: name.to_string(),
                        path: path.to_string(),
                        event: event.to_string(),
                        timeout,
                    }),
                    _ => problems.push(format!(
                        "line {n}: expected `hook <agent> {target} <Event> [timeout]`"
                    )),
                }
            }
            ("hook", _) => problems.push(format!(
                "line {n}: `hook` needs a #path suffix. Use `hook <agent> {slug}#<path> <Event>`"
            )),
            ("skill", _) => problems.push(format!(
                "line {n}: `skill` needs a #skill suffix. Use `skill {slug}#<skill-name>`"
            )),
            (other, _) => problems.push(format!("line {n}: unknown kind `{other}`")),
        }
    }
    (entries, problems)
}

/// Make checkout paths absolute so links and hook commands resolve consistently.
pub(crate) fn src_root(ctx: &Ctx) -> PathBuf {
    let p = std::env::var_os("DOTFILES_SRC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| ctx.home.join("dev"));
    std::path::absolute(&p).unwrap_or(p)
}

/// Checkout path for a repo. The owner segment is lowercased so
/// `Ethan-Arrowood/agentic` lands beside the existing `~/dev/ethan-arrowood/`.
fn checkout_path(ctx: &Ctx, owner: &str, name: &str) -> PathBuf {
    src_root(ctx).join(owner.to_lowercase()).join(name)
}

/// Clone missing repos. Leave existing working checkouts alone.
pub(crate) fn ensure_clone(ctx: &Ctx, owner: &str, name: &str) -> Result<Option<PathBuf>> {
    let dest = checkout_path(ctx, owner, name);
    let slug = format!("{owner}/{name}");

    // .git is a file, not a directory, in a worktree or a submodule.
    if dest.join(".git").exists() {
        ui::skip(&format!("{slug} already cloned at {}", ctx.pretty(&dest)));
        return Ok(Some(dest));
    }
    if dest.exists() {
        ui::warn(&format!(
            "{} exists but is not a git checkout; skipping {slug}",
            ctx.pretty(&dest)
        ));
        return Ok(None);
    }
    if ctx.dry_run {
        ui::dry(&format!("clone {slug} -> {}", ctx.pretty(&dest)));
        return Ok(None);
    }

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    ui::step(&format!("cloning {slug} into {}", ctx.pretty(&dest)));
    if !first_success(ctx, &slug, &clone_attempts(&slug, &dest))? {
        return Ok(None);
    }
    Ok(Some(dest))
}

/// Try HTTPS without prompts for fresh machines lacking credentials.
/// Then try gh for private repos, then SSH.
fn clone_attempts(slug: &str, dest: &Path) -> Vec<Cmd> {
    let dest = dest.to_string_lossy();
    let mut attempts = vec![Cmd::new("git")
        .args([
            "clone",
            "--quiet",
            &format!("https://github.com/{slug}.git"),
        ])
        .arg(dest.clone())
        .env("GIT_TERMINAL_PROMPT", "0")];
    if sys::has("gh") {
        attempts.push(
            Cmd::new("gh")
                .args(["repo", "clone", slug])
                .arg(dest.clone()),
        );
    }
    attempts.push(
        Cmd::new("git")
            .args(["clone", "--quiet", &format!("git@github.com:{slug}.git")])
            .arg(dest),
    );
    attempts
}

/// Run attempts in order until one succeeds. Capture their output so a failed
/// clone prints one warning instead of every tool's errors. `--verbose` shows
/// each attempt's output when all of them fail.
fn first_success(ctx: &Ctx, slug: &str, attempts: &[Cmd]) -> Result<bool> {
    let mut failed = Vec::new();
    for cmd in attempts {
        let out = cmd.capture()?;
        if out.ok {
            return Ok(true);
        }
        failed.push((cmd.display(), out.combined()));
    }
    let reason = failed
        .last()
        .and_then(|(_, out)| failure_reason(out))
        .unwrap_or("no reason given");
    if ctx.verbose {
        ui::warn(&format!("could not clone {slug}: {reason}"));
        for (cmd, out) in &failed {
            ui::detail(&format!("$ {cmd}"));
            ui::detail(out.trim_end());
        }
    } else {
        ui::warn(&format!(
            "could not clone {slug}: {reason} (--verbose shows every attempt)"
        ));
    }
    Ok(false)
}

/// The first line that explains a failed clone, skipping git's progress line.
fn failure_reason(output: &str) -> Option<&str> {
    output
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("Cloning into"))
}

/// Find `SKILL.md` through `max_depth` levels. Stop inside a skill so nested
/// examples and fixtures do not become separate skills.
pub fn discover(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, max: usize, out: &mut Vec<PathBuf>) {
        if depth > max {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut dirs: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .filter(|p| {
                // Skip dot-directories: .git, .github, .idea and friends.
                !p.file_name()
                    .map(|n| n.to_string_lossy().starts_with('.'))
                    .unwrap_or(true)
            })
            .collect();
        dirs.sort();
        for d in dirs {
            if d.join("SKILL.md").is_file() {
                out.push(d);
            } else {
                walk(&d, depth + 1, max, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, 1, max_depth, &mut out);
    out
}

const MAX_DEPTH: usize = 2;

/// Track claimed skill names across layers to prevent relinking back and forth
/// and colliding with earlier backups.
pub type Claimed = std::collections::HashMap<String, String>;

pub fn install(ctx: &Ctx, layer: &Layer, claimed: &mut Claimed) -> Result<()> {
    let path = layer.path(ctx).join("Skillfile");
    if !path.is_file() {
        return Ok(());
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let (entries, problems) = parse(&text);
    for p in &problems {
        ui::warn(&format!("Skillfile: {p}"));
    }
    if entries.is_empty() {
        return Ok(());
    }

    ui::step("installing skills");
    let dest_dir = ctx.home.join(".claude/skills");
    if !ctx.dry_run {
        std::fs::create_dir_all(&dest_dir)?;
    }

    // Several entries usually name the same repo; clone (or report) it once.
    let mut checkouts: std::collections::HashMap<String, Option<PathBuf>> =
        std::collections::HashMap::new();

    for entry in &entries {
        let checkout = match checkouts.get(&entry.slug()) {
            Some(c) => c.clone(),
            None => {
                let c = ensure_clone(ctx, entry.owner(), entry.name())?;
                checkouts.insert(entry.slug(), c.clone());
                c
            }
        };
        let Some(checkout) = checkout else {
            continue;
        };
        match entry {
            Entry::Rules { .. } => {
                claude::link_rules(ctx, &checkout.join("rules"), &src_root(ctx))?
            }
            Entry::Hook {
                agent,
                path,
                event,
                timeout,
                ..
            } => {
                let script = checkout.join(path);
                if script.is_file() {
                    hooks::register(ctx, *agent, &script, event, *timeout)?;
                } else {
                    ui::warn(&format!("hook script {} not found", ctx.pretty(&script)));
                }
            }
            Entry::Repo { .. } => {
                let found = discover(&checkout, MAX_DEPTH);
                if found.is_empty() {
                    ui::warn(&format!("no SKILL.md found in {}", ctx.pretty(&checkout)));
                }
                for skill_dir in found {
                    let name = skill_dir
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned();
                    if let Some(other) = claimed.get(&name) {
                        ui::warn(&format!(
                            "skill '{name}' is declared by both {other} and {}; keeping {other}",
                            entry.slug()
                        ));
                        continue;
                    }
                    claimed.insert(name.clone(), entry.slug());
                    link::link_file(ctx, &skill_dir, &dest_dir.join(&name))?;
                }
            }
            Entry::Skill { skill, .. } => {
                match discover(&checkout, MAX_DEPTH)
                    .into_iter()
                    .find(|p| p.file_name().map(|n| n == skill.as_str()).unwrap_or(false))
                {
                    Some(skill_dir) => {
                        if let Some(other) = claimed.get(skill) {
                            ui::warn(&format!(
                                "skill '{skill}' is declared by both {other} and {}; keeping {other}",
                                entry.slug()
                            ));
                            continue;
                        }
                        claimed.insert(skill.clone(), entry.slug());
                        link::link_file(ctx, &skill_dir, &dest_dir.join(skill))?
                    }
                    None => ui::warn(&format!(
                        "skill '{skill}' not found in {}",
                        ctx.pretty(&checkout)
                    )),
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hook_entries() {
        let (e, p) = parse(
            "hook claude o/r#agents/claude/sync.sh SessionStart 15   # note\n\
             hook codex o/r#a.sh PreToolUse\n\
             hook claude o/r#a.sh\n\
             hook claude o/r SessionStart\n\
             hook claude o/r#a.sh SessionStart soon\n\
             hook o/r#a.sh SessionStart\n\
             hook\n",
        );
        assert_eq!(
            e,
            vec![
                Entry::Hook {
                    agent: Agent::Claude,
                    owner: "o".into(),
                    name: "r".into(),
                    path: "agents/claude/sync.sh".into(),
                    event: "SessionStart".into(),
                    timeout: Some(15),
                },
                Entry::Hook {
                    agent: Agent::Codex,
                    owner: "o".into(),
                    name: "r".into(),
                    path: "a.sh".into(),
                    event: "PreToolUse".into(),
                    timeout: None,
                },
            ]
        );
        assert_eq!(p.len(), 5, "{p:?}");
        assert!(p[3].contains("unknown agent `o/r#a.sh`"), "{}", p[3]);
    }

    #[test]
    fn parses_rules_entries() {
        let (e, p) = parse("rules Ethan-Arrowood/agentic\nrules a/b#c\n");
        assert_eq!(
            e,
            vec![Entry::Rules {
                owner: "Ethan-Arrowood".into(),
                name: "agentic".into()
            }]
        );
        assert_eq!(p.len(), 1, "{p:?}");
    }

    #[test]
    fn parses_repo_and_skill_entries() {
        let (e, p) = parse(
            "repo Ethan-Arrowood/agentic      # everything in it\n\
             skill harperfast/skills#harper-best-practices\n",
        );
        assert!(p.is_empty(), "{p:?}");
        assert_eq!(
            e,
            vec![
                Entry::Repo {
                    owner: "Ethan-Arrowood".into(),
                    name: "agentic".into()
                },
                Entry::Skill {
                    owner: "harperfast".into(),
                    name: "skills".into(),
                    skill: "harper-best-practices".into()
                },
            ]
        );
    }

    /// The `#` that separates repo from skill must survive comment stripping.
    #[test]
    fn a_hash_inside_a_token_is_not_a_comment() {
        let (e, p) = parse("skill a/b#c   # trailing note\n");
        assert!(p.is_empty(), "{p:?}");
        assert_eq!(
            e,
            vec![Entry::Skill {
                owner: "a".into(),
                name: "b".into(),
                skill: "c".into()
            }]
        );
        assert_eq!(strip_comment("skill a/b#c  # note").trim(), "skill a/b#c");
        assert_eq!(strip_comment("# whole line").trim(), "");
        assert_eq!(strip_comment("repo a/b").trim(), "repo a/b");
    }

    #[test]
    fn comments_and_blanks_are_ignored() {
        let (e, p) = parse("\n# a comment\n\n   \nrepo a/b\n");
        assert_eq!(e.len(), 1);
        assert!(p.is_empty());
    }

    #[test]
    fn malformed_lines_are_reported_not_silently_dropped() {
        let (e, p) = parse(
            "repo justaname\n\
             skill a/b\n\
             repo a/b#c\n\
             bogus a/b\n\
             repo good/one\n\
             repo a/b/c\n\
             repo a/b extra\n",
        );
        assert_eq!(
            e,
            vec![Entry::Repo {
                owner: "good".into(),
                name: "one".into()
            }]
        );
        assert_eq!(p.len(), 6, "{p:?}");
        assert!(p[0].contains("line 1"));
        assert!(p[1].contains("line 2") && p[1].contains("#skill"));
        assert!(p[2].contains("line 3") && p[2].contains("no #skill"));
        assert!(p[3].contains("line 4") && p[3].contains("unknown kind"));
    }

    #[test]
    fn owner_directory_is_lowercased_to_match_existing_layout() {
        let ctx = Ctx::new(std::env::temp_dir()).unwrap();
        let p = checkout_path(&ctx, "Ethan-Arrowood", "agentic");
        assert!(p.ends_with("ethan-arrowood/agentic"), "{}", p.display());
    }

    #[test]
    fn failure_reason_is_the_first_explaining_line() {
        let ssh = "Cloning into 'x'...\n\
                   git@github.com: Permission denied (publickey).\n\
                   fatal: Could not read from remote repository.\n\n\
                   Please make sure you have the correct access rights\n\
                   and the repository exists.\n";
        assert_eq!(
            failure_reason(ssh),
            Some("git@github.com: Permission denied (publickey).")
        );
        assert_eq!(
            failure_reason("To get started with GitHub CLI, please run:  gh auth login\n"),
            Some("To get started with GitHub CLI, please run:  gh auth login")
        );
        assert_eq!(failure_reason("\nCloning into 'x'...\n"), None);
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = Cmd::new("git")
            .args(["-C", &dir.to_string_lossy()])
            .args(args.iter().copied())
            .capture()
            .unwrap();
        assert!(out.ok, "git {args:?}: {}", out.combined());
    }

    /// Failed attempts are captured; the first success stops the sequence.
    #[test]
    fn falls_back_quietly_until_an_attempt_succeeds() {
        let tmp = crate::tmpdir::PrivateTempDir::new("clonetest").unwrap();
        let origin = tmp.join("origin.git");
        std::fs::create_dir(&origin).unwrap();
        git(&origin, &["init", "-q", "--bare"]);
        let missing = format!("file://{}", tmp.join("missing.git").display());
        let clone = |url: &str, dest: &str| {
            Cmd::new("git")
                .args(["clone", "--quiet", url])
                .arg(tmp.join(dest).to_string_lossy())
        };
        let ctx = Ctx::new(tmp.path().to_path_buf()).unwrap();

        let attempts = [
            clone(&missing, "a"),
            clone(&format!("file://{}", origin.display()), "a"),
            clone(&missing, "b"),
        ];
        assert!(first_success(&ctx, "o/r", &attempts).unwrap());
        assert!(tmp.join("a/.git").is_dir());
        assert!(
            !tmp.join("b").exists(),
            "attempts after a success must not run"
        );

        let attempts = [clone(&missing, "c"), clone(&missing, "c")];
        assert!(!first_success(&ctx, "o/r", &attempts).unwrap());
        assert!(!tmp.join("c").exists());
    }

    /// Support both top-level skills and skills/ layouts.
    #[test]
    fn discovers_skills_at_depth_one_and_two() {
        let tmp = std::env::temp_dir().join(format!("dfskills{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);

        // depth 1, like harperfast/skills
        std::fs::create_dir_all(tmp.join("top-level-skill")).unwrap();
        std::fs::write(tmp.join("top-level-skill/SKILL.md"), "").unwrap();
        // depth 2, like Ethan-Arrowood/agentic and harperfast/skills-internal
        std::fs::create_dir_all(tmp.join("skills/nested-skill")).unwrap();
        std::fs::write(tmp.join("skills/nested-skill/SKILL.md"), "").unwrap();
        // must be ignored
        std::fs::create_dir_all(tmp.join(".github/workflows")).unwrap();
        std::fs::create_dir_all(tmp.join("docs")).unwrap();
        // a nested SKILL.md inside a skill must not become its own entry
        std::fs::create_dir_all(tmp.join("top-level-skill/examples")).unwrap();
        std::fs::write(tmp.join("top-level-skill/examples/SKILL.md"), "").unwrap();

        let mut found: Vec<String> = discover(&tmp, 2)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        found.sort();
        assert_eq!(found, vec!["nested-skill", "top-level-skill"]);

        std::fs::remove_dir_all(&tmp).ok();
    }
}
