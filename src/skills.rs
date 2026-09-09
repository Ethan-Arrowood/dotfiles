//! Skills declared per layer, in a `Skillfile`.
//!
//! Skills are not vendored into this repo: their source lives in its own
//! repository, which gets cloned once and then symlinked into
//! `~/.claude/skills/`. Claude Code follows a symlinked skill directory and
//! reads `SKILL.md` from the target, so the clone is simultaneously the
//! installed skill and an editable checkout -- edit it in place and the change
//! is live, no re-sync step.
//!
//! Format mirrors the Brewfile: one entry per line, `#` comments allowed.
//!
//!   repo  <owner>/<name>              clone it, link every skill it contains
//!   skill <owner>/<name>#<skill>      clone it, link just that one skill
//!
//! Repos are cloned to `$DOTFILES_SRC_DIR/<owner>/<name>`, default `~/dev`,
//! matching the existing `~/dev/<owner>/<repo>` layout.

use crate::ctx::Ctx;
use crate::layer::Layer;
use crate::sys::{self, Cmd};
use crate::{link, ui};
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
}

impl Entry {
    pub fn owner(&self) -> &str {
        match self {
            Entry::Repo { owner, .. } | Entry::Skill { owner, .. } => owner,
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Entry::Repo { name, .. } | Entry::Skill { name, .. } => name,
        }
    }
}

/// Strip a trailing `#` comment without eating the `#` that separates a repo
/// from a skill name. A `#` only starts a comment at the start of a line or
/// after whitespace, so `skill a/b#c  # note` keeps `skill a/b#c`.
fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    for (i, &c) in bytes.iter().enumerate() {
        if c == b'#' && (i == 0 || bytes[i - 1].is_ascii_whitespace()) {
            return &line[..i];
        }
    }
    line
}

/// Parse a Skillfile. Malformed lines are reported with their line number
/// rather than skipped silently.
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
        let (Some(kind), Some(target)) = (parts.next(), parts.next()) else {
            problems.push(format!(
                "line {n}: expected `<kind> <owner>/<repo>`, got `{line}`"
            ));
            continue;
        };

        // owner/repo, optionally #skill
        let (slug, skill) = match target.split_once('#') {
            Some((s, k)) => (s, Some(k)),
            None => (target, None),
        };
        let Some((owner, name)) = slug.split_once('/') else {
            problems.push(format!("line {n}: `{slug}` is not <owner>/<repo>"));
            continue;
        };
        if owner.is_empty() || name.is_empty() {
            problems.push(format!("line {n}: `{slug}` is not <owner>/<repo>"));
            continue;
        }

        match (kind, skill) {
            ("repo", None) => entries.push(Entry::Repo {
                owner: owner.to_string(),
                name: name.to_string(),
            }),
            ("repo", Some(_)) => problems.push(format!(
                "line {n}: `repo` takes no #skill suffix — use `skill {target}`"
            )),
            ("skill", Some(k)) if !k.is_empty() => entries.push(Entry::Skill {
                owner: owner.to_string(),
                name: name.to_string(),
                skill: k.to_string(),
            }),
            ("skill", _) => problems.push(format!(
                "line {n}: `skill` needs a #skill suffix — use `skill {slug}#<skill-name>`"
            )),
            (other, _) => problems.push(format!("line {n}: unknown kind `{other}`")),
        }
    }
    (entries, problems)
}

/// Where repo checkouts live.
fn src_root(ctx: &Ctx) -> PathBuf {
    std::env::var_os("DOTFILES_SRC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| ctx.home.join("dev"))
}

/// Checkout path for a repo. The owner segment is lowercased so
/// `Ethan-Arrowood/skills` lands beside the existing `~/dev/ethan-arrowood/`.
fn checkout_path(ctx: &Ctx, owner: &str, name: &str) -> PathBuf {
    src_root(ctx).join(owner.to_lowercase()).join(name)
}

/// Clone if absent, otherwise leave it alone -- these are working checkouts
/// the user edits, so pulling could stomp on in-flight work.
fn ensure_clone(ctx: &Ctx, owner: &str, name: &str) -> Result<Option<PathBuf>> {
    let dest = checkout_path(ctx, owner, name);
    let slug = format!("{owner}/{name}");

    if dest.join(".git").is_dir() {
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

    // gh handles auth for private and internal repos; plain git works once the
    // core layer's ssh key is registered.
    let ok = if sys::has("gh") {
        Cmd::new("gh")
            .args(["repo", "clone", &slug])
            .arg(dest.to_string_lossy())
            .run(ctx, &format!("gh repo clone {slug}"))?
    } else {
        Cmd::new("git")
            .arg("clone")
            .arg(format!("git@github.com:{slug}.git"))
            .arg(dest.to_string_lossy())
            .run(ctx, &format!("git clone {slug}"))?
    };
    if !ok {
        ui::warn(&format!(
            "could not clone {slug} (private? not authenticated?)"
        ));
        return Ok(None);
    }
    Ok(Some(dest))
}

/// Directories containing a `SKILL.md`, searched to `max_depth`.
///
/// Bounded on purpose: the three repos in use put skills at depth 1
/// (`<skill>/SKILL.md`) or depth 2 (`skills/<skill>/SKILL.md`), and unlimited
/// recursion would pick up example or fixture SKILL.md files nested inside a
/// skill. Descent stops as soon as a directory is identified as a skill.
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

pub fn install(ctx: &Ctx, layer: &Layer) -> Result<()> {
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

    for entry in &entries {
        let Some(checkout) = ensure_clone(ctx, entry.owner(), entry.name())? else {
            continue;
        };
        match entry {
            Entry::Repo { .. } => {
                let found = discover(&checkout, MAX_DEPTH);
                if found.is_empty() {
                    ui::warn(&format!("no SKILL.md found in {}", ctx.pretty(&checkout)));
                }
                for skill_dir in found {
                    let name = skill_dir.file_name().unwrap().to_owned();
                    link::link_file(ctx, &skill_dir, &dest_dir.join(name))?;
                }
            }
            Entry::Skill { skill, .. } => {
                match discover(&checkout, MAX_DEPTH)
                    .into_iter()
                    .find(|p| p.file_name().map(|n| n == skill.as_str()).unwrap_or(false))
                {
                    Some(skill_dir) => link::link_file(ctx, &skill_dir, &dest_dir.join(skill))?,
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
    fn parses_repo_and_skill_entries() {
        let (e, p) = parse(
            "repo Ethan-Arrowood/skills      # everything in it\n\
             skill harperfast/skills#harper-best-practices\n",
        );
        assert!(p.is_empty(), "{p:?}");
        assert_eq!(
            e,
            vec![
                Entry::Repo {
                    owner: "Ethan-Arrowood".into(),
                    name: "skills".into()
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
             repo good/one\n",
        );
        assert_eq!(
            e,
            vec![Entry::Repo {
                owner: "good".into(),
                name: "one".into()
            }]
        );
        assert_eq!(p.len(), 4, "{p:?}");
        assert!(p[0].contains("line 1"));
        assert!(p[1].contains("line 2") && p[1].contains("#skill"));
        assert!(p[2].contains("line 3") && p[2].contains("no #skill"));
        assert!(p[3].contains("line 4") && p[3].contains("unknown kind"));
    }

    #[test]
    fn owner_directory_is_lowercased_to_match_existing_layout() {
        let ctx = Ctx::new(std::env::temp_dir(), true, true).unwrap();
        let p = checkout_path(&ctx, "Ethan-Arrowood", "skills");
        assert!(p.ends_with("ethan-arrowood/skills"), "{}", p.display());
    }

    /// The three repos in use have three different layouts, so discovery must
    /// find SKILL.md rather than assume a path.
    #[test]
    fn discovers_skills_at_depth_one_and_two() {
        let tmp = std::env::temp_dir().join(format!("dfskills{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);

        // depth 1, like Ethan-Arrowood/skills and harperfast/skills
        std::fs::create_dir_all(tmp.join("top-level-skill")).unwrap();
        std::fs::write(tmp.join("top-level-skill/SKILL.md"), "").unwrap();
        // depth 2, like harperfast/skills-internal
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
