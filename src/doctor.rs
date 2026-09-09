//! Check the current machine for problems.

use crate::ctx::Ctx;
use crate::signing;
use crate::sys::{self, Cmd};
use crate::ui;
use anyhow::Result;

#[derive(Default)]
struct Report {
    problems: usize,
}

impl Report {
    fn ok(&self, msg: &str) {
        ui::ok(msg);
    }
    fn skip(&self, msg: &str) {
        ui::skip(msg);
    }
    fn warn(&mut self, msg: &str) {
        ui::warn(msg);
        self.problems += 1;
    }
    fn error(&mut self, msg: &str) {
        ui::error(msg);
        self.problems += 1;
    }
}

pub fn run(ctx: &Ctx) -> Result<bool> {
    let mut r = Report::default();
    ui::head("Doctor");

    for tool in ["git", "zsh"] {
        match sys::which(tool) {
            Some(p) => r.ok(&format!("{tool} {}", p.display())),
            None => r.error(&format!("{tool} not installed")),
        }
    }

    let shell = std::env::var("SHELL").unwrap_or_default();
    if shell.ends_with("zsh") {
        r.ok("login shell is zsh");
    } else if sys::is_container() {
        r.skip(&format!(
            "login shell is {shell} (containers keep the image default)"
        ));
    } else {
        r.warn(&format!(
            "login shell is {shell}, not zsh — run: chsh -s $(command -v zsh)"
        ));
    }

    // ssh
    let key = signing::ssh_key_path(ctx);
    if key.exists() {
        r.ok(&format!("ssh key {}", ctx.pretty(&key)));
        let fp = Cmd::new("ssh-keygen")
            .arg("-lf")
            .arg(key.to_string_lossy())
            .capture()
            .ok()
            .and_then(|c| c.stdout.split_whitespace().nth(1).map(String::from));
        let loaded = fp
            .as_ref()
            .and_then(|f| {
                Cmd::new("ssh-add")
                    .arg("-l")
                    .capture()
                    .ok()
                    .map(|c| c.stdout.contains(f.as_str()))
            })
            .unwrap_or(false);
        if loaded {
            r.ok("ssh key loaded in the agent");
        } else {
            r.warn(&format!(
                "ssh key is not in the agent — run: ssh-add {}",
                ctx.pretty(&key)
            ));
        }
    } else {
        r.warn(&format!("no ssh key at {}", ctx.pretty(&key)));
    }

    let ssh_config = ctx.home.join(".ssh/config");
    if std::fs::read_to_string(&ssh_config)
        .map(|t| t.contains("config.d/"))
        .unwrap_or(false)
    {
        r.ok("~/.ssh/config includes config.d/");
    } else {
        r.warn("~/.ssh/config does not include config.d/ — run: dotfiles install");
    }

    if key.exists() {
        let probe = Cmd::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=8",
                "-T",
                "git@github.com",
            ])
            .capture()
            .map(|c| c.combined())
            .unwrap_or_default();
        if probe.contains("successfully authenticated") {
            r.ok("github ssh authentication works");
        } else if probe.contains("Permission denied") {
            r.warn("github rejected the ssh key — run: dotfiles github");
        } else {
            r.skip("github ssh probe inconclusive (offline?)");
        }
    }

    // signing
    let signing_key = Cmd::new("git")
        .args(["config", "--global", "user.signingkey"])
        .capture()
        .map(|c| c.trimmed().to_string())
        .unwrap_or_default();
    let format = Cmd::new("git")
        .args(["config", "--global", "gpg.format"])
        .capture()
        .map(|c| c.trimmed().to_string())
        .unwrap_or_else(|_| "openpgp".into());

    if signing_key.is_empty() {
        r.skip("no signing key configured");
    } else if format == "ssh" {
        let path = std::path::PathBuf::from(shellexpand_home(ctx, &signing_key));
        if path.exists() {
            r.ok(&format!("signing with ssh key {}", ctx.pretty(&path)));
            if ctx.home.join(".ssh/allowed_signers").exists() {
                r.ok("allowed_signers present (local verification works)");
            } else {
                r.warn("no ~/.ssh/allowed_signers — git can sign but not verify");
            }
        } else {
            r.error(&format!(
                "git signs with {signing_key} but that file does not exist"
            ));
        }
    } else if signing::gpg_key_present(&signing_key) {
        r.ok(&format!("gpg signing key {signing_key} present"));
        if let Some(days) = gpg_days_left(&signing_key) {
            if days < 0 {
                r.error(&format!("signing key EXPIRED {} days ago", -days));
            } else if days < 60 {
                // r.warn, not ui::warn: this has to affect the exit code, or
                // `doctor` reports success on a key that is about to expire.
                r.warn(&format!("signing key expires in {days} days"));
            } else {
                r.ok(&format!("signing key valid for {days} more days"));
            }
        }
    } else {
        r.warn(&format!(
            "git is configured to sign with {signing_key} but it is not in the keyring"
        ));
    }

    if sys::is_macos() {
        match crate::brew::brew_bin() {
            Some(p) => r.ok(&format!("homebrew {}", p.display())),
            None => r.error("homebrew missing"),
        }
        if Cmd::new("xcode-select")
            .arg("-p")
            .capture()
            .map(|c| c.ok)
            .unwrap_or(false)
        {
            r.ok("xcode command line tools");
        } else {
            r.error("xcode command line tools missing");
        }
    }

    // Symlinks pointing at a repo that has since moved.
    for name in [".zshrc", ".zshenv", ".zprofile", ".gitconfig"] {
        let p = ctx.home.join(name);
        if std::fs::symlink_metadata(&p).is_ok() && !p.exists() {
            r.error(&format!(
                "{} is a broken symlink -> {}",
                ctx.pretty(&p),
                std::fs::read_link(&p).unwrap_or_default().display()
            ));
        }
    }

    println!();
    if r.problems == 0 {
        ui::ok("no problems found");
        Ok(true)
    } else {
        ui::warn(&format!("{} problem(s) found", r.problems));
        Ok(false)
    }
}

fn shellexpand_home(ctx: &Ctx, p: &str) -> String {
    match p.strip_prefix("~/") {
        Some(rest) => ctx.home.join(rest).display().to_string(),
        None => p.to_string(),
    }
}

/// Days until the gpg key expires, from the colon-delimited listing.
fn gpg_days_left(key: &str) -> Option<i64> {
    let out = Cmd::new("gpg")
        .args(["--list-secret-keys", "--with-colons", key])
        .capture()
        .ok()?;
    let expiry: i64 = out
        .stdout
        .lines()
        .find(|l| l.starts_with("sec:"))
        .and_then(|l| l.split(':').nth(6).map(String::from))
        .filter(|s| !s.is_empty() && s != "0")?
        .parse()
        .ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    Some((expiry - now) / 86400)
}
