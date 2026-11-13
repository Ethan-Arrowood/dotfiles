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
            "login shell is {shell}, not zsh. Run: chsh -s $(command -v zsh)"
        ));
    }

    // ssh
    let key = signing::ssh_key_path(ctx);
    let fingerprint = key
        .exists()
        .then(|| {
            Cmd::new("ssh-keygen")
                .arg("-lf")
                .arg(key.to_string_lossy())
                .capture()
                .ok()
                .and_then(|c| c.stdout.split_whitespace().nth(1).map(String::from))
        })
        .flatten();
    // Fails with no agent or an empty one.
    let agent_list = Cmd::new("ssh-add")
        .arg("-l")
        .capture()
        .ok()
        .filter(|c| c.ok)
        .map(|c| c.stdout)
        .unwrap_or_default();
    let auth = ssh_auth(key.exists(), fingerprint.as_deref(), &agent_list);
    match auth {
        SshAuth::KeyLoaded => {
            r.ok(&format!("ssh key {}", ctx.pretty(&key)));
            r.ok("ssh key loaded in the agent");
        }
        SshAuth::KeyNotLoaded => {
            r.ok(&format!("ssh key {}", ctx.pretty(&key)));
            r.warn(&format!(
                "ssh key is not in the agent. Run: ssh-add {}",
                ctx.pretty(&key)
            ));
        }
        SshAuth::AgentOnly(n) => r.ok(&format!(
            "no key file at {}, but the ssh agent has {n} key(s) (forwarded?)",
            ctx.pretty(&key)
        )),
        SshAuth::None => r.warn(&format!("no ssh key at {}", ctx.pretty(&key))),
    }

    let ssh_config = ctx.home.join(".ssh/config");
    if std::fs::read_to_string(&ssh_config)
        .map(|t| crate::layers::core::includes_config_d(&t))
        .unwrap_or(false)
    {
        r.ok("~/.ssh/config includes config.d/");
    } else {
        r.warn("~/.ssh/config does not include config.d/. Run: dotfiles install");
    }

    if auth != SshAuth::None {
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
            r.warn("github rejected the ssh key. Run: dotfiles github");
        } else {
            r.skip("github ssh probe inconclusive (offline?)");
        }
    }

    // signing
    let signing_key = Cmd::new("git")
        .args(["config", "--global", "--includes", "user.signingkey"])
        .capture()
        .map(|c| c.trimmed().to_string())
        .unwrap_or_default();
    let format = Cmd::new("git")
        .args(["config", "--global", "--includes", "gpg.format"])
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
                r.warn("no ~/.ssh/allowed_signers: git can sign but not verify");
            }
        } else {
            r.error(&format!(
                "git signs with {signing_key} but that file does not exist"
            ));
        }
    } else {
        r.warn(&format!(
            "git is configured to sign with {signing_key}, but gpg signing was removed; \
             re-run `dotfiles install` to switch to the ssh key"
        ));
    }

    if crate::ncu::config_missing(ctx) {
        r.warn("node-core-utils has no config where ncu looks. Run: dotfiles ncu");
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
    ctx.expand(p).display().to_string()
}

/// Where ssh credentials come from.
#[derive(Debug, PartialEq)]
enum SshAuth {
    KeyLoaded,
    KeyNotLoaded,
    /// No key file, but the agent lists keys: a forwarded agent in a
    /// container, for example. Holds the number of keys.
    AgentOnly(usize),
    None,
}

/// `agent_list` is the output of a successful `ssh-add -l`, empty otherwise.
fn ssh_auth(key_exists: bool, fingerprint: Option<&str>, agent_list: &str) -> SshAuth {
    if key_exists {
        match fingerprint {
            Some(fp) if agent_list.contains(fp) => SshAuth::KeyLoaded,
            _ => SshAuth::KeyNotLoaded,
        }
    } else {
        match agent_list.lines().filter(|l| !l.trim().is_empty()).count() {
            0 => SshAuth::None,
            n => SshAuth::AgentOnly(n),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = "256 SHA256:abc me@example.com (ED25519)\n";

    #[test]
    fn key_file_in_agent() {
        assert_eq!(ssh_auth(true, Some("SHA256:abc"), LIST), SshAuth::KeyLoaded);
    }

    #[test]
    fn key_file_not_in_agent() {
        assert_eq!(
            ssh_auth(true, Some("SHA256:xyz"), LIST),
            SshAuth::KeyNotLoaded
        );
        assert_eq!(
            ssh_auth(true, Some("SHA256:abc"), ""),
            SshAuth::KeyNotLoaded
        );
        assert_eq!(ssh_auth(true, None, LIST), SshAuth::KeyNotLoaded);
    }

    #[test]
    fn forwarded_agent_without_key_file() {
        assert_eq!(ssh_auth(false, None, LIST), SshAuth::AgentOnly(1));
        let two = format!("{LIST}256 SHA256:def other@example.com (ED25519)\n");
        assert_eq!(ssh_auth(false, None, &two), SshAuth::AgentOnly(2));
    }

    #[test]
    fn no_key_file_and_no_agent_keys() {
        assert_eq!(ssh_auth(false, None, ""), SshAuth::None);
        assert_eq!(ssh_auth(false, None, "\n"), SshAuth::None);
    }
}
