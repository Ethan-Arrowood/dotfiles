//! Layers and profiles.
//!
//! A layer is a named unit of setup: some Rust that performs its steps, plus
//! a directory of assets (`home/`, `shell/`, `zsh/`, `ssh/`, `Brewfile`, ...).
//! The registry below is the single source of truth for which layers exist,
//! so there is no metadata file to drift out of sync with the code.

use crate::ctx::Ctx;
use crate::sys::Os;
use anyhow::{bail, Context, Result};
use std::collections::BTreeSet;
use std::path::PathBuf;

pub type InstallFn = fn(&Ctx, &Layer) -> Result<()>;

pub struct Layer {
    /// Stable name. Used on the command line, in profiles, and as the
    /// directory name under `layers/`.
    pub name: &'static str,
    pub description: &'static str,
    pub platforms: &'static [Os],
    pub install: InstallFn,
}

impl Layer {
    pub fn path(&self, ctx: &Ctx) -> PathBuf {
        ctx.root.join("layers").join(self.name)
    }
    pub fn supports(&self, os: Os) -> bool {
        self.platforms.is_empty() || self.platforms.contains(&os)
    }
    pub fn platform_label(&self) -> String {
        if self.platforms.is_empty() {
            "any".into()
        } else {
            self.platforms
                .iter()
                .map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        }
    }
}

pub const ANY: &[Os] = &[];
pub const MACOS: &[Os] = &[Os::MacOs];

/// Every layer. **Declaration order is install order** -- and the order
/// shell fragments are sourced in. There is no separate order field and no
/// numeric prefix on the directories: one ordered list, one source of truth.
pub fn registry() -> Vec<Layer> {
    use crate::layers;
    vec![
        Layer {
            name: "core",
            description: "zsh + git + ssh. Portable base for any machine or container.",
            platforms: ANY,
            install: layers::core::install,
        },
        Layer {
            name: "mac-base",
            description: "Xcode CLT, Homebrew, CLI tools, macOS system defaults.",
            platforms: MACOS,
            install: layers::mac_base::install,
        },
        Layer {
            name: "mac-langs",
            description: "Node (fnm), Rust (rustup), Python (uv), C/C++ build tools.",
            platforms: MACOS,
            install: layers::mac_langs::install,
        },
        Layer {
            name: "mac-apps",
            description: "Essential apps: Chrome, VS Code, Claude, Ghostty, Dashlane CLI.",
            platforms: MACOS,
            install: layers::mac_apps::install,
        },
        Layer {
            name: "mac-apps-extra",
            description: "Quality-of-life apps: Slack, Zoom, Spotify, Discord, containers.",
            platforms: MACOS,
            install: layers::mac_apps_extra::install,
        },
        Layer {
            name: "work-harper",
            description: "Harper dev-server aliases + LastPass. Opt-in only.",
            platforms: ANY,
            install: layers::work_harper::install,
        },
    ]
}

pub fn find(name: &str) -> Option<Layer> {
    registry().into_iter().find(|l| l.name == name)
}

pub fn names() -> Vec<&'static str> {
    registry().iter().map(|l| l.name).collect()
}

/// A profile is an ordered set of layer names, stored as one name per line.
pub fn profile_layers(ctx: &Ctx, profile: &str) -> Result<Vec<String>> {
    let path = ctx.root.join("profiles").join(profile);
    let Ok(text) = std::fs::read_to_string(&path) else {
        let available = all_profiles(ctx).unwrap_or_default().join(" ");
        bail!("unknown profile '{profile}' (available: {available})");
    };
    Ok(text
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

pub fn all_profiles(ctx: &Ctx) -> Result<Vec<String>> {
    let dir = ctx.root.join("profiles");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    Ok(names)
}

/// profile ∪ `with` ∖ `without`, ordered by the registry, de-duplicated.
///
/// Unknown names in `with`/`without` are an error: a typo must not quietly
/// install one layer fewer than asked for.
pub fn resolve(
    ctx: &Ctx,
    profile: &str,
    with: &[String],
    without: &[String],
) -> Result<Vec<Layer>> {
    let known = names();
    for n in with.iter().chain(without.iter()) {
        if !known.contains(&n.as_str()) {
            bail!("unknown layer '{n}' (available: {})", known.join(" "));
        }
    }

    let mut wanted: BTreeSet<String> = profile_layers(ctx, profile)?.into_iter().collect();
    for n in with {
        wanted.insert(n.clone());
    }
    for n in without {
        wanted.remove(n);
    }

    // Filtering the registry preserves its declaration order, which is the
    // install order -- no separate sort needed.
    let selected: Vec<Layer> = registry()
        .into_iter()
        .filter(|l| wanted.remove(l.name))
        .collect();
    // Anything left in `wanted` was named by a profile but is not a real layer.
    if let Some(unknown) = wanted.iter().next() {
        bail!("profile '{profile}' names unknown layer '{unknown}'");
    }

    if selected.is_empty() {
        bail!("no layers selected");
    }
    // Every other layer's shell fragments call helpers defined in core's
    // 10-env.sh (path_prepend), so dropping core leaves them erroring on every
    // shell start.
    if !selected.iter().any(|l| l.name == "core") {
        bail!(
            "'core' cannot be dropped: the other layers' shell fragments depend on it \
             (path_prepend and friends)"
        );
    }
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_names_are_unique() {
        let r = registry();
        let mut names: Vec<&str> = r.iter().map(|l| l.name).collect();
        let count = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), count, "duplicate layer names");
    }

    #[test]
    fn core_is_always_first() {
        // Everything else assumes zsh, git and ssh are already in place.
        assert_eq!(registry().first().unwrap().name, "core");
    }

    #[test]
    fn every_layer_has_an_asset_directory() {
        // The name IS the directory now, so a typo would silently produce a
        // layer that installs nothing.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        for l in registry() {
            let dir = root.join("layers").join(l.name);
            assert!(dir.is_dir(), "layer '{}' has no {}", l.name, dir.display());
        }
    }

    #[test]
    fn core_cannot_be_dropped() {
        let ctx = Ctx::new(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
            true,
            true,
        )
        .unwrap();
        let err = match resolve(&ctx, "mac-dev", &[], &["core".to_string()]) {
            Ok(_) => panic!("dropping core must be refused"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("core"), "{err}");
    }

    #[test]
    fn core_runs_anywhere_and_mac_layers_do_not() {
        assert!(find("core").unwrap().supports(Os::Linux));
        assert!(!find("mac-base").unwrap().supports(Os::Linux));
        assert!(find("work-harper").unwrap().supports(Os::Linux));
    }
}
