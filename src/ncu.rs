//! `dotfiles ncu`: set up node-core-utils (`ncu-ci`, `git-node`) on this
//! machine, one token of each kind per machine, like the ssh key.
//!
//! GitHub has no API to create a personal access token, and Jenkins tokens
//! need a signed-in browser session, so each token takes one visit to a web
//! page. Everything around those visits is done here.
//!
//! `ncu-config set` echoes the old and new values and creates the file
//! world-readable. So tokens are read here at a hidden prompt, passed to
//! `ncu-config` on stdin with its output discarded, and the file is made 0600.
//! They are never printed or written anywhere but the config file.

use crate::ctx::Ctx;
use crate::sys::{self, Cmd};
use crate::ui;
use anyhow::{bail, Context, Result};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Where ncu reads its global config: `$XDG_CONFIG_HOME/ncurc` whenever that
/// variable is set, which the core layer's `.zshenv` always does.
pub fn config_dir(ctx: &Ctx) -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| ctx.home.join(".config"))
}

fn ncu_config_bin(ctx: &Ctx) -> Option<PathBuf> {
    let shared = ctx.home.join(".npm-global/bin/ncu-config");
    if shared.is_file() {
        return Some(shared);
    }
    sys::which("ncu-config")
}

/// node-core-utils is installed but has no config where ncu will look.
pub fn config_missing(ctx: &Ctx) -> bool {
    let dir = config_dir(ctx);
    ncu_config_bin(ctx).is_some() && !dir.join("ncurc").exists() && !dir.join("ncurc.gpg").exists()
}

/// Key names present in the config. Values are dropped unread.
fn keys(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.as_object().map(|m| m.keys().cloned().collect()))
        .unwrap_or_default()
}

pub fn run(ctx: &Ctx) -> Result<()> {
    ui::head("node-core-utils setup");
    let Some(bin) = ncu_config_bin(ctx) else {
        bail!("ncu-config is not installed. Run `dotfiles install`; mac-langs installs @node-core/utils");
    };
    if !sys::has("fnm") {
        bail!("fnm is not installed; ncu-config needs its Node");
    }
    let dir = config_dir(ctx);
    let cfg = dir.join("ncurc");
    if dir.join("ncurc.gpg").exists() {
        ui::skip("encrypted ncurc.gpg found; leaving it alone");
        return Ok(());
    }
    std::fs::create_dir_all(&dir)?;

    // A config made before XDG_CONFIG_HOME was set lives at ~/.ncurc, which
    // ncu stops reading once the variable is set.
    let legacy = ctx.home.join(".ncurc");
    if !cfg.exists() && legacy.is_file() && !legacy.is_symlink() {
        std::fs::rename(&legacy, &cfg).context("moving ~/.ncurc")?;
        // A shell without XDG_CONFIG_HOME still reads ~/.ncurc, so leave a
        // link there until every shell gets the core .zshenv.
        std::os::unix::fs::symlink(&cfg, &legacy).context("linking ~/.ncurc")?;
        ui::ok(&format!(
            "moved {} to {}, where ncu now looks, and linked the old path to it",
            ctx.pretty(&legacy),
            ctx.pretty(&cfg)
        ));
    }

    let have = keys(&cfg);
    let needs_tokens =
        !have.iter().any(|k| k == "token") || !have.iter().any(|k| k == "jenkins_token");
    if needs_tokens && !sys::has_terminal() {
        bail!("no terminal to read tokens from; run `dotfiles ncu` in a terminal");
    }
    let has = |k: &str| have.iter().any(|h| h == k);

    if has("username") {
        ui::skip("username already set");
    } else {
        let login = Cmd::new("gh")
            .args(["api", "user", "--jq", ".login"])
            .capture()
            .ok()
            .filter(|c| c.ok)
            .map(|c| c.trimmed().to_string())
            .filter(|l| !l.is_empty());
        let Some(login) = login else {
            bail!("could not read your GitHub login. Run `gh auth login` first");
        };
        set(&bin, &dir, "username", &login)?;
        ui::ok(&format!("username set to {login}"));
    }
    let login = login_from(&cfg).unwrap_or_default();

    if has("token") {
        ui::skip("GitHub token already set");
    } else {
        let url = format!(
            "https://github.com/settings/tokens/new?scopes=user:email,read:org&description={}",
            url_encode(&format!("ncu: {}", machine_name()))
        );
        ui::step("GitHub token: scopes user:email and read:org are pre-selected");
        ui::detail("pick an expiry, click Generate token, and copy it");
        open(&url);
        let token = read_secret("GitHub token")?;
        set(&bin, &dir, "token", &token)?;
        ui::ok("GitHub token set");
    }

    if has("jenkins_token") {
        ui::skip("Jenkins token already set");
    } else {
        let url = format!("https://ci.nodejs.org/user/{login}/security");
        ui::step(&format!(
            "Jenkins token: under API Token, Add new token, name it \"{}\", Generate, and copy it",
            machine_name()
        ));
        open(&url);
        let token = read_secret("Jenkins token")?;
        set(&bin, &dir, "jenkins_token", &token)?;
        ui::ok("Jenkins token set");
    }

    if cfg.exists() {
        crate::link::chmod(&cfg, 0o600)?;
    }
    ui::detail(&format!(
        "to replace a token later, delete its key from {} and re-run `dotfiles ncu`",
        ctx.pretty(&cfg)
    ));
    Ok(())
}

/// Run `ncu-config set -g <key>` with the value on stdin and its output, which
/// echoes old and new values, discarded.
fn set(bin: &Path, dir: &Path, key: &str, value: &str) -> Result<()> {
    let mut child = Command::new("fnm")
        .args(["exec", "--using=default", "--"])
        .arg(bin)
        .args(["set", "-g", key])
        .env("XDG_CONFIG_HOME", dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("running ncu-config")?;
    if let Some(mut stdin) = child.stdin.take() {
        writeln!(stdin, "{value}")?;
    }
    if !child.wait()?.success() {
        bail!("ncu-config set {key} failed");
    }
    let path = dir.join("ncurc");
    if path.exists() {
        crate::link::chmod(&path, 0o600)?;
    }
    Ok(())
}

fn login_from(cfg: &Path) -> Option<String> {
    let text = std::fs::read_to_string(cfg).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v["username"].as_str().map(String::from)
}

/// Read a line from the terminal with echo off.
fn read_secret(label: &str) -> Result<String> {
    let tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .context("no terminal to read the token from")?;
    let stty = |arg: &str| {
        let _ = Command::new("stty")
            .arg(arg)
            .stdin(tty.try_clone().map(Stdio::from).unwrap_or(Stdio::null()))
            .status();
    };
    let mut out = tty.try_clone()?;
    write!(out, "    ? paste the {label} (hidden): ")?;
    out.flush()?;
    stty("-echo");
    let mut line = String::new();
    let read = std::io::BufReader::new(tty.try_clone()?).read_line(&mut line);
    stty("echo");
    writeln!(out)?;
    read?;
    let value = line.trim().to_string();
    if value.is_empty() {
        bail!("no {label} entered");
    }
    Ok(value)
}

/// Open a URL in the browser, and print it in case that doesn't work. Over
/// ssh the browser would open on the far machine, so only print it there.
fn open(url: &str) {
    ui::detail(url);
    if std::env::var_os("SSH_CONNECTION").is_some() {
        return;
    }
    let opener = if sys::is_macos() { "open" } else { "xdg-open" };
    let _ = Cmd::new(opener).arg(url).capture();
}

fn machine_name() -> String {
    let name = if sys::is_macos() {
        Cmd::new("scutil")
            .args(["--get", "ComputerName"])
            .capture()
            .ok()
    } else {
        Cmd::new("hostname").capture().ok()
    };
    name.filter(|c| c.ok)
        .map(|c| c.trimmed().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "this machine".into())
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_encode_escapes_spaces_and_colons() {
        assert_eq!(url_encode("ncu: Ethan's Mac"), "ncu%3A%20Ethan%27s%20Mac");
    }

    /// Key names only; the values never leave `keys`.
    #[test]
    fn keys_reads_names_and_missing_config_is_detected() {
        let dir = crate::tmpdir::PrivateTempDir::new("ncutest").unwrap();
        let mut ctx = Ctx::new(dir.join("repo")).unwrap();
        ctx.home = dir.join("home");
        let bin = ctx.home.join(".npm-global/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("ncu-config"), "").unwrap();
        if std::env::var_os("XDG_CONFIG_HOME").is_none() {
            assert!(config_missing(&ctx));
            // ~/.ncurc alone doesn't count: ncu won't read it.
            std::fs::write(ctx.home.join(".ncurc"), "{}").unwrap();
            assert!(config_missing(&ctx));
            let cfg = config_dir(&ctx).join("ncurc");
            std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
            std::fs::write(&cfg, r#"{"username":"u","token":"t"}"#).unwrap();
            assert!(!config_missing(&ctx));
            let mut k = keys(&cfg);
            k.sort();
            assert_eq!(k, ["token", "username"]);
        }
    }
}
