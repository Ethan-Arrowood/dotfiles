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
    envs: Vec<(String, String)>,
    quiet: bool,
}

impl Cmd {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            envs: Vec::new(),
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
    pub fn env(mut self, key: impl Into<String>, val: impl Into<String>) -> Self {
        self.envs.push((key.into(), val.into()));
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
        for (k, v) in &self.envs {
            c.env(k, v);
        }
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
        let mut c = Command::new(&self.program);
        c.args(&self.args);
        for (k, v) in &self.envs {
            c.env(k, v);
        }
        let out: Output = c
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
    // Guarded here rather than at the call site: this escalates to root and
    // mutates the machine, so it must not depend on a caller remembering.
    if !ctx.dry_run && !allow_global_changes() {
        return Ok(false);
    }
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

/// Whether machine-global changes are allowed.
///
/// $HOME differing from the account's real home means a test harness with a
/// sandboxed $HOME. File operations follow $HOME and stay contained; package
/// installs, `defaults write`, `chsh` and the toolchain installers do not, and
/// would escape the sandbox onto the real machine.
///
/// Fails CLOSED. An earlier version returned "not sandboxed" from every error
/// path, so a machine where the home lookup failed -- a directory-services
/// bound Mac, a container without `id` -- silently ran with no guard at all.
/// For a safety check, "I could not tell" must mean "do not".
/// DOTFILES_ALLOW_SANDBOXED_HOME=1 is the escape hatch.
pub fn home_is_sandboxed() -> Sandbox {
    if std::env::var_os("DOTFILES_ALLOW_SANDBOXED_HOME").is_some() {
        return Sandbox::No;
    }
    let Some(env_home) = dirs::home_dir() else {
        return Sandbox::Unknown("$HOME is not set");
    };
    let Some(real_home) = real_home_dir() else {
        return Sandbox::Unknown("could not read this account's home directory");
    };
    // Compare resolved paths: a symlinked or trailing-slash $HOME is the same
    // home, and should not read as a sandbox.
    let canon = |p: &std::path::Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    if canon(&env_home) == canon(&real_home) {
        Sandbox::No
    } else {
        Sandbox::Yes
    }
}

pub enum Sandbox {
    /// $HOME is the account's real home; global changes are fine.
    No,
    /// $HOME is somewhere else; this is a test run.
    Yes,
    /// Could not determine. Treated as sandboxed.
    Unknown(&'static str),
}

/// The home directory from the account database, ignoring $HOME.
fn real_home_dir() -> Option<std::path::PathBuf> {
    let user = current_username()?;

    if let Ok(o) = Command::new("getent").args(["passwd", &user]).output() {
        if o.status.success() {
            let line = String::from_utf8_lossy(&o.stdout).into_owned();
            if let Some(dir) = line.split(':').nth(5) {
                if !dir.is_empty() {
                    return Some(std::path::PathBuf::from(dir));
                }
            }
        }
    }

    // macOS has no getent. Query /Search rather than the local node `.`, so a
    // machine bound to AD/LDAP -- where the user record is not local -- still
    // resolves instead of silently disabling the guard.
    for node in ["/Search", "."] {
        let Ok(o) = Command::new("dscl")
            .args([node, "-read", &format!("/Users/{user}"), "NFSHomeDirectory"])
            .output()
        else {
            continue;
        };
        if !o.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&o.stdout);
        // Either "NFSHomeDirectory: /Users/x" or the key on its own line with
        // the value on the next -- the latter when the value contains a space.
        let value = text
            .strip_prefix("NFSHomeDirectory:")
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .or_else(|| {
                text.lines()
                    .nth(1)
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_string)
            });
        if let Some(v) = value {
            return Some(std::path::PathBuf::from(v));
        }
    }
    None
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
/// Reports at most once per run; it is consulted from a dozen call sites and
/// repeating the same line a dozen times is noise.
pub fn allow_global_changes() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    static REPORTED: AtomicBool = AtomicBool::new(false);

    let (blocked, why) = match home_is_sandboxed() {
        Sandbox::No => return true,
        Sandbox::Yes => (true, "sandboxed $HOME detected".to_string()),
        Sandbox::Unknown(reason) => (true, format!("cannot verify $HOME ({reason})")),
    };
    if blocked && !REPORTED.swap(true, Ordering::Relaxed) {
        crate::ui::skip(&format!(
            "{why}: skipping machine-global changes \
             (set DOTFILES_ALLOW_SANDBOXED_HOME=1 to override)"
        ));
    }
    false
}

/// The login shell recorded in the account database, which is what chsh
/// changes. $SHELL only reflects the value at login time.
pub fn login_shell_of_record() -> Option<String> {
    let user = current_username()?;
    if let Ok(o) = Command::new("getent").args(["passwd", &user]).output() {
        if o.status.success() {
            let line = String::from_utf8_lossy(&o.stdout).into_owned();
            if let Some(sh) = line.trim_end().rsplit(':').next() {
                if !sh.is_empty() {
                    return Some(sh.to_string());
                }
            }
        }
    }
    for node in ["/Search", "."] {
        let Ok(o) = Command::new("dscl")
            .args([node, "-read", &format!("/Users/{user}"), "UserShell"])
            .output()
        else {
            continue;
        };
        if !o.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&o.stdout);
        if let Some(v) = text.strip_prefix("UserShell:").map(str::trim) {
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Download a script, then run it -- never `curl … | sh`.
///
/// A pipeline's exit status is the shell's, so a failed download exits 0 and
/// the caller's error branch never fires. `set -o pipefail` would fix it on
/// bash but /bin/sh is dash on many Linuxes. Downloading first also means a
/// truncated transfer cannot execute half a script.
///
/// `envs` is how an installer is told not to edit shell rc files.
pub fn run_remote_script(
    ctx: &Ctx,
    desc: &str,
    url: &str,
    interpreter: &str,
    script_args: &[&str],
    envs: &[(&str, &str)],
) -> Result<bool> {
    if ctx.dry_run {
        crate::ui::dry(desc);
        return Ok(true);
    }
    let dir = crate::tmpdir::PrivateTempDir::new("installer")?;
    let script = dir.join("install.sh");

    let downloaded = Cmd::new("curl")
        .args(["-fsSL", url, "-o"])
        .arg(script.to_string_lossy())
        .run(ctx, &format!("downloading {url}"))?;
    if !downloaded {
        crate::ui::warn(&format!("could not download {url}"));
        return Ok(false);
    }

    let mut cmd = Cmd::new(interpreter).arg(script.to_string_lossy());
    for a in script_args {
        cmd = cmd.arg(*a);
    }
    for (k, v) in envs {
        cmd = cmd.env(*k, *v);
    }
    cmd.run(ctx, desc)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_home_resolves_on_this_machine() {
        // If this fails the guard would previously have silently switched off;
        // now it fails closed instead, but either way it is worth knowing.
        assert!(
            real_home_dir().is_some(),
            "could not read this account's home directory"
        );
    }

    #[test]
    fn unknown_is_treated_as_sandboxed() {
        // The direction that matters: "cannot tell" must block, not allow.
        let unknown = Sandbox::Unknown("test");
        assert!(
            matches!(unknown, Sandbox::Unknown(_)),
            "Unknown must exist as a distinct state"
        );
        // And the override must still win.
        std::env::set_var("DOTFILES_ALLOW_SANDBOXED_HOME", "1");
        assert!(matches!(home_is_sandboxed(), Sandbox::No));
        std::env::remove_var("DOTFILES_ALLOW_SANDBOXED_HOME");
    }

    #[test]
    fn a_trailing_slash_home_is_not_a_sandbox() {
        let Some(real) = real_home_dir() else { return };
        let orig = std::env::var_os("HOME");
        std::env::set_var("HOME", format!("{}/", real.display()));
        let verdict = matches!(home_is_sandboxed(), Sandbox::No);
        match orig {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        assert!(
            verdict,
            "a trailing slash on $HOME must not read as sandboxed"
        );
    }

    #[test]
    fn login_shell_of_record_is_readable() {
        assert!(login_shell_of_record().is_some_and(|s| s.starts_with('/')));
    }
}
