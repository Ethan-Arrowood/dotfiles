//! The Dock, set from every selected layer's `Dockfile`.
//!
//! Runs once after all layers, not inside one: mac-base runs before mac-apps
//! installs Chrome, so applying it there would leave Chrome out until the
//! next run.

use crate::ctx::Ctx;
use crate::layer::Layer;
use crate::sys::{self, Cmd};
use crate::{skills, ui};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq)]
struct Item {
    path: PathBuf,
    /// dockutil flags, e.g. `--view fan`.
    flags: Vec<String>,
}

impl Item {
    fn is_app(&self) -> bool {
        self.path.extension().is_some_and(|e| e == "app")
    }
}

/// One item per line: a path, optionally followed by dockutil flags. The
/// flags start at the first ` --`, so paths may contain spaces.
fn parse(ctx: &Ctx, text: &str) -> Vec<Item> {
    text.lines()
        .map(|l| skills::strip_comment(l).trim())
        .filter(|l| !l.is_empty())
        .map(|l| {
            let (path, flags) = match l.find(" --") {
                Some(i) => (&l[..i], &l[i..]),
                None => (l, ""),
            };
            Item {
                path: ctx.expand(path.trim()),
                flags: flags.split_whitespace().map(String::from).collect(),
            }
        })
        .collect()
}

/// Decode `%20` and friends in a `file://` URL.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The paths in one Dock section, from `defaults read`'s `_CFURLString`s.
fn paths_in(plist: &str) -> Vec<PathBuf> {
    plist
        .lines()
        .filter_map(|l| l.trim().strip_prefix("\"_CFURLString\" = \""))
        .filter_map(|l| l.strip_suffix("\";"))
        .filter_map(|url| url.strip_prefix("file://"))
        .map(|p| PathBuf::from(percent_decode(p.trim_end_matches('/'))))
        .collect()
}

/// Resolved, so the Dock storing `/Applications/Safari.app` (a symlink into
/// the Safari cryptex) or its target cannot make every run rewrite the Dock.
fn resolved(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn current(section: &str) -> Vec<PathBuf> {
    Cmd::new("defaults")
        .args(["read", "com.apple.dock", section])
        .capture()
        .map(|c| paths_in(&c.stdout).iter().map(|p| resolved(p)).collect())
        .unwrap_or_default()
}

pub fn apply(ctx: &Ctx, layers: &[Layer]) -> Result<()> {
    if !sys::is_macos() {
        return Ok(());
    }
    let mut items = Vec::new();
    for l in layers {
        let path = l.path(ctx).join("Dockfile");
        if path.is_file() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            items.extend(parse(ctx, &text));
        }
    }
    if items.is_empty() {
        return Ok(());
    }
    ui::head("Dock");
    if !ctx.dry_run && !sys::allow_global_changes() {
        return Ok(());
    }
    items.retain(|i| {
        if !i.path.is_absolute() {
            ui::warn(&format!(
                "Dockfile: `{}` is not an absolute or ~/ path; skipping it",
                i.path.display()
            ));
            return false;
        }
        let present = i.path.exists();
        if !present {
            ui::skip(&format!("{} (not installed)", ctx.pretty(&i.path)));
        }
        present
    });

    let (apps, others): (Vec<&Item>, Vec<&Item>) = items.iter().partition(|i| i.is_app());
    let want = |v: &[&Item]| v.iter().map(|i| resolved(&i.path)).collect::<Vec<_>>();
    // The Dock's plist stores view and sort settings as numbers that don't
    // map one to one onto dockutil's flags, so compare paths there and the
    // flags against what the last successful run applied.
    let spec = spec(&items);
    let applied = ctx.state_dir.join("dock.applied");
    if current("persistent-apps") == want(&apps)
        && current("persistent-others") == want(&others)
        && std::fs::read_to_string(&applied).is_ok_and(|t| t == spec)
    {
        ui::skip("Dock already set");
        return Ok(());
    }
    if !ctx.dry_run && !sys::has("dockutil") {
        ui::warn("dockutil not installed (mac-base's Brewfile has it); leaving the Dock alone");
        return Ok(());
    }

    let mut failed = !Cmd::new("dockutil")
        .args(["--remove", "all", "--no-restart"])
        .quiet()
        .run(ctx, "clear the Dock")?;
    for i in apps.iter().chain(&others) {
        let section = if i.is_app() { "apps" } else { "others" };
        failed |= !Cmd::new("dockutil")
            .arg("--add")
            .arg(i.path.to_string_lossy())
            .args(["--section", section])
            .args(i.flags.iter().cloned())
            .arg("--no-restart")
            .quiet()
            .run(ctx, &format!("add {} to the Dock", ctx.pretty(&i.path)))?;
    }
    if !ctx.dry_run {
        let _ = Cmd::new("killall").arg("Dock").capture();
        if failed {
            // Not recorded, so the next run tries again.
            ui::warn("some dockutil commands failed; the Dock may be incomplete");
        } else {
            std::fs::create_dir_all(&ctx.state_dir)?;
            std::fs::write(&applied, &spec)?;
            ui::ok("Dock set");
        }
    }
    Ok(())
}

/// The Dock as the Dockfiles describe it, one item per line, for comparing
/// with the last applied run.
fn spec(items: &[Item]) -> String {
    items
        .iter()
        .map(|i| format!("{} {}\n", i.path.display(), i.flags.join(" ")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_paths_with_spaces_and_flags() {
        let ctx = Ctx::new(PathBuf::from("/repo")).unwrap();
        let items = parse(
            &ctx,
            "# comment\n\
             /System/Applications/System Settings.app\n\
             \n\
             ~/Downloads   --view fan --sort dateadded   # trailing note\n",
        );
        assert_eq!(
            items,
            vec![
                Item {
                    path: "/System/Applications/System Settings.app".into(),
                    flags: vec![],
                },
                Item {
                    path: ctx.home.join("Downloads"),
                    flags: ["--view", "fan", "--sort", "dateadded"]
                        .map(String::from)
                        .to_vec(),
                },
            ]
        );
        assert!(items[0].is_app());
        assert!(!items[1].is_app());
    }

    #[test]
    fn reads_paths_back_out_of_the_dock_plist() {
        let plist = r#"(
        {
            "tile-data" =         {
                "file-data" =             {
                    "_CFURLString" = "file:///System/Applications/System%20Settings.app/";
                    "_CFURLStringType" = 15;
                };
            };
        },
        {
            "tile-data" =         {
                "file-data" =             {
                    "_CFURLString" = "file:///Users/me/Downloads/";
                };
            };
        }
)"#;
        assert_eq!(
            paths_in(plist),
            vec![
                PathBuf::from("/System/Applications/System Settings.app"),
                PathBuf::from("/Users/me/Downloads"),
            ]
        );
    }

    #[test]
    fn percent_decode_leaves_a_stray_percent_alone() {
        assert_eq!(percent_decode("a%20b%zz%"), "a b%zz%");
    }
}
