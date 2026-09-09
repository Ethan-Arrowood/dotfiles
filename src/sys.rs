//! Platform detection and subprocess execution.

use crate::ctx::Ctx;
use crate::ui;
use anyhow::Result;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub enum Os {
    MacOs,
    Linux,
    Other,
}

impl Os {
    pub fn current() -> Self {
        match std::env::consts::OS {
            "macos" => Os::MacOs,
            "linux" => Os::Linux,
            _ => Os::Other,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Os::MacOs => "macos",
            Os::Linux => "linux",
            Os::Other => "other",
        }
    }
}

pub fn is_macos() -> bool {
    Os::current() == Os::MacOs
}

/// Inside a container / devcontainer / CI, where prompting and GUI installs
/// make no sense and the host's ssh agent is usually forwarded in.
pub fn is_container() -> bool {
    for v in ["REMOTE_CONTAINERS", "CODESPACES", "DEVCONTAINER"] {
        if std::env::var_os(v).is_some() {
            return true;
        }
    }
    std::path::Path::new("/.dockerenv").exists()
        || std::path::Path::new("/run/.containerenv").exists()
}

/// Is `name` on PATH?
pub fn has(name: &str) -> bool {
    which(name).is_some()
}

pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let full = dir.join(name);
        // Executable and not a directory.
        if full.is_file() && is_executable(&full) {
            Some(full)
        } else {
            None
        }
    })
}

#[cfg(unix)]
fn is_executable(p: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// A command to run, with the description shown to the user.
pub struct Cmd {
    program: String,
    args: Vec<String>,
    quiet: bool,
}

impl Cmd {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            quiet: false,
        }
    }
    pub fn arg(mut self, a: impl Into<String>) -> Self {
        self.args.push(a.into());
        self
    }
    pub fn args<I, S>(mut self, items: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(items.into_iter().map(Into::into));
        self
    }
    /// Suppress the child's stdout/stderr.
    pub fn quiet(mut self) -> Self {
        self.quiet = true;
        self
    }

    pub fn display(&self) -> String {
        if self.args.is_empty() {
            self.program.clone()
        } else {
            format!("{} {}", self.program, self.args.join(" "))
        }
    }

    /// Run it, honoring dry-run. `Ok(true)` means it ran and succeeded;
    /// `Ok(false)` means it ran and failed. Only a failure to *spawn* is an
    /// `Err` -- a non-zero exit is data, not an error, because most of these
    /// steps are advisory.
    pub fn run(&self, ctx: &Ctx, desc: &str) -> Result<bool> {
        if ctx.dry_run {
            ui::dry(desc);
            return Ok(true);
        }
        ui::step(desc);
        let mut c = Command::new(&self.program);
        c.args(&self.args);
        if self.quiet {
            c.stdout(Stdio::null()).stderr(Stdio::null());
        }
        let status = c
            .status()
            .map_err(|e| anyhow::anyhow!("could not run `{}`: {e}", self.display()))?;
        Ok(status.success())
    }

    /// Capture stdout+stderr. Never fails on a non-zero exit; the caller
    /// inspects `Capture::ok`.
    pub fn capture(&self) -> Result<Capture> {
        let out: Output = Command::new(&self.program)
            .args(&self.args)
            .output()
            .map_err(|e| anyhow::anyhow!("could not run `{}`: {e}", self.display()))?;
        Ok(Capture {
            ok: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }
}

pub struct Capture {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

impl Capture {
    pub fn combined(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
    pub fn trimmed(&self) -> &str {
        self.stdout.trim()
    }
}

/// Run as root: already root, or via sudo, or refuse. Containers usually run
/// as root with no sudo installed, so shelling out to a missing sudo would be
/// the wrong failure.
pub fn as_root(ctx: &Ctx, desc: &str, program: &str, args: &[&str]) -> Result<bool> {
    let am_root = unsafe { libc_geteuid() } == 0;
    if am_root {
        Cmd::new(program).args(args.iter().copied()).run(ctx, desc)
    } else if has("sudo") {
        Cmd::new("sudo")
            .arg(program)
            .args(args.iter().copied())
            .run(ctx, desc)
    } else {
        ui::warn(&format!(
            "need root for: {program} {} (no sudo available)",
            args.join(" ")
        ));
        Ok(false)
    }
}

// Avoiding a libc dependency for one call.
#[cfg(unix)]
unsafe fn libc_geteuid() -> u32 {
    extern "C" {
        fn geteuid() -> u32;
    }
    geteuid()
}

/// Best-effort system package install, used only to get zsh and git present.
pub fn sys_install(ctx: &Ctx, pkgs: &[&str]) -> Result<bool> {
    let list = pkgs.join(" ");
    if has("apt-get") {
        as_root(ctx, "apt-get update", "apt-get", &["update", "-qq"])?;
        let mut args = vec!["install", "-y", "--no-install-recommends"];
        args.extend_from_slice(pkgs);
        as_root(ctx, &format!("apt-get install {list}"), "apt-get", &args)
    } else if has("dnf") {
        let mut args = vec!["install", "-y"];
        args.extend_from_slice(pkgs);
        as_root(ctx, &format!("dnf install {list}"), "dnf", &args)
    } else if has("apk") {
        let mut args = vec!["add", "--no-cache"];
        args.extend_from_slice(pkgs);
        as_root(ctx, &format!("apk add {list}"), "apk", &args)
    } else if has("pacman") {
        let mut args = vec!["-S", "--noconfirm"];
        args.extend_from_slice(pkgs);
        as_root(ctx, &format!("pacman -S {list}"), "pacman", &args)
    } else if has("brew") {
        Cmd::new("brew")
            .arg("install")
            .args(pkgs.iter().copied())
            .run(ctx, &format!("brew install {list}"))
    } else {
        ui::warn(&format!(
            "no supported package manager; install manually: {list}"
        ));
        Ok(false)
    }
}

/// True when $HOME is not this user's real home directory.
///
/// That means we are running under a test harness with a sandboxed HOME.
/// File operations follow $HOME and are safely contained, but `brew install`
/// and `defaults write` are machine-global and would escape the sandbox --
/// so those are skipped unless explicitly allowed.
pub fn home_is_sandboxed() -> bool {
    if std::env::var_os("DOTFILES_ALLOW_SANDBOXED_HOME").is_some() {
        return false;
    }
    let Some(env_home) = dirs::home_dir() else {
        return false;
    };
    let Some(real_home) = real_home_dir() else {
        return false;
    };
    env_home != real_home
}

/// The home directory from the password database, ignoring $HOME.
fn real_home_dir() -> Option<std::path::PathBuf> {
    let out = Command::new("getent")
        .args(["passwd", &current_username()?])
        .output()
        .ok();
    if let Some(o) = out {
        if o.status.success() {
            let line = String::from_utf8_lossy(&o.stdout).into_owned();
            if let Some(dir) = line.split(':').nth(5) {
                return Some(std::path::PathBuf::from(dir));
            }
        }
    }
    // macOS has no getent; dscl is the equivalent.
    let o = Command::new("dscl")
        .args([
            ".",
            "-read",
            &format!("/Users/{}", current_username()?),
            "NFSHomeDirectory",
        ])
        .output()
        .ok()?;
    if !o.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&o.stdout);
    text.split_whitespace().nth(1).map(std::path::PathBuf::from)
}

fn current_username() -> Option<String> {
    Command::new("id")
        .arg("-un")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Guard for anything that changes machine-global state.
///
/// Returns false (and explains) when running with a sandboxed HOME, so a test
/// run cannot install packages or rewrite system preferences for real.
pub fn allow_global_changes() -> bool {
    if home_is_sandboxed() {
        crate::ui::skip(
            "sandboxed $HOME detected: skipping machine-global changes              (set DOTFILES_ALLOW_SANDBOXED_HOME=1 to override)",
        );
        return false;
    }
    true
}

pub fn hostname_short() -> String {
    if is_macos() {
        if let Ok(c) = Cmd::new("scutil").args(["--get", "ComputerName"]).capture() {
            if c.ok && !c.trimmed().is_empty() {
                return c.trimmed().to_string();
            }
        }
    }
    Cmd::new("hostname")
        .arg("-s")
        .capture()
        .ok()
        .filter(|c| c.ok)
        .map(|c| c.trimmed().to_string())
        .unwrap_or_else(|| "unknown".into())
}
