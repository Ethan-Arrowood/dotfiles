//! One command for the GitHub key dance, so nobody rereads the docs.
//!
//! GitHub treats these as three separate uploads, which is the part that is
//! easy to forget:
//!
//! - ssh AUTHENTICATION key (git push) -- scope `admin:public_key`
//! - ssh SIGNING key (Verified badge) -- scope `admin:ssh_signing_key`
//! - gpg key (Verified badge) -- scope `write:gpg_key`
//!
//! The same ssh key must be uploaded twice, once as each type.

use crate::ctx::Ctx;
use crate::signing::{self, GPG_KEY};
use crate::sys::{self, Cmd};
use crate::tmpdir::PrivateTempDir;
use crate::ui;
use anyhow::{bail, Result};

fn gh_authenticated() -> bool {
    Cmd::new("gh")
        .args(["auth", "status"])
        .capture()
        .map(|c| c.ok)
        .unwrap_or(false)
}

fn key_title() -> String {
    std::env::var("DOTFILES_KEY_TITLE").unwrap_or_else(|_| sys::hostname_short())
}

/// Ensure the token carries a scope, refreshing if not.
fn ensure_scope(ctx: &Ctx, want: &str) -> bool {
    let status = match Cmd::new("gh").args(["auth", "status"]).capture() {
        Ok(c) => c.combined(),
        Err(_) => return false,
    };
    let have: Vec<String> = status
        .lines()
        .find_map(|l| l.split("Token scopes:").nth(1))
        .map(|s| {
            s.split(',')
                .map(|t| t.trim().trim_matches('\'').to_string())
                .collect()
        })
        .unwrap_or_default();
    if have.iter().any(|s| s == want) {
        return true;
    }
    ui::warn(&format!("your gh token is missing the '{want}' scope"));
    if !ui::confirm(
        &format!("Refresh the token to add '{want}'? (opens a browser)"),
        ctx.assume_yes,
    ) {
        return false;
    }
    Cmd::new("gh")
        .args(["auth", "refresh", "-h", "github.com", "-s", want])
        .run(ctx, &format!("gh auth refresh -s {want}"))
        .unwrap_or(false)
}

/// Is this public key already registered, for the given type?
/// `None` means "cannot tell" (usually a missing scope), which must not be
/// treated as "absent".
fn registered(pubkey_body: &str, kind: Option<&str>) -> Option<bool> {
    let mut cmd = Cmd::new("gh").args(["ssh-key", "list"]);
    if let Some(k) = kind {
        cmd = cmd.args(["--type", k]);
    }
    let out = cmd.capture().ok()?;
    if !out.ok {
        return None;
    }
    Some(out.stdout.contains(pubkey_body))
}

pub fn run(ctx: &Ctx) -> Result<()> {
    ui::head("GitHub key setup");
    if !sys::has("gh") {
        bail!("gh is not installed (brew install gh)");
    }
    if !gh_authenticated() {
        ui::warn("gh is not authenticated");
        if !ui::confirm("Run 'gh auth login' now?", ctx.assume_yes) {
            bail!("gh auth login is required first");
        }
        if !Cmd::new("gh")
            .args(["auth", "login"])
            .run(ctx, "gh auth login")?
        {
            bail!("gh auth login failed");
        }
    }
    let who = Cmd::new("gh")
        .args(["api", "user", "--jq", ".login"])
        .capture()
        .map(|c| c.trimmed().to_string())
        .unwrap_or_else(|_| "?".into());
    ui::ok(&format!("authenticated as {who}"));

    let key = signing::ssh_key_path(ctx);
    let pub_path = key.with_extension("pub");
    if !pub_path.exists() {
        bail!("no ssh key at {} — run: dotfiles install", ctx.pretty(&key));
    }
    let Some(pubkey) = signing::ssh_pub_key(ctx) else {
        bail!("could not read {}", ctx.pretty(&pub_path));
    };
    let body = pubkey
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_string();

    ui::head(&format!("SSH key: {}", ctx.pretty(&key)));
    add_ssh(ctx, &pub_path, &body, None, &key_title())?;

    // Only offer the signing upload when git actually signs with ssh.
    let format = Cmd::new("git")
        .args(["config", "--global", "gpg.format"])
        .capture()
        .map(|c| c.trimmed().to_string())
        .unwrap_or_default();
    if format == "ssh" {
        add_ssh(
            ctx,
            &pub_path,
            &body,
            Some("signing"),
            &format!("{} (signing)", key_title()),
        )?;
    } else {
        ui::skip("git signs with gpg, not ssh — skipping the ssh signing key");
    }

    ui::head(&format!("GPG key: {GPG_KEY}"));
    add_gpg(ctx)?;

    verify(ctx);
    Ok(())
}

fn add_ssh(
    ctx: &Ctx,
    pub_path: &std::path::Path,
    body: &str,
    kind: Option<&str>,
    title: &str,
) -> Result<()> {
    let label = kind.unwrap_or("authentication");
    match registered(body, kind) {
        Some(true) => {
            ui::skip(&format!("ssh {label} key already on github"));
            return Ok(());
        }
        Some(false) => {}
        None => ui::warn(&format!(
            "cannot list {label} keys (missing scope?); will ask before uploading"
        )),
    }

    let scope = match kind {
        Some("signing") => "admin:ssh_signing_key",
        _ => "admin:public_key",
    };
    if !ensure_scope(ctx, scope) {
        ui::warn(&format!("skipping {label} key upload"));
        return Ok(());
    }
    // Re-check now that the scope may have been granted.
    if registered(body, kind) == Some(true) {
        ui::skip(&format!("ssh {label} key already on github"));
        return Ok(());
    }

    let question = match kind {
        Some("signing") => {
            "Upload the SAME key again as a SIGNING key? (github treats these separately)"
                .to_string()
        }
        _ => format!("Upload {} as an AUTHENTICATION key?", ctx.pretty(pub_path)),
    };
    if !ui::confirm(&question, ctx.assume_yes) {
        ui::skip("declined");
        return Ok(());
    }

    let mut cmd = Cmd::new("gh")
        .args(["ssh-key", "add"])
        .arg(pub_path.to_string_lossy());
    if let Some(k) = kind {
        cmd = cmd.args(["--type", k]);
    }
    cmd = cmd.args(["--title", title]);
    if cmd.run(ctx, &format!("gh ssh-key add ({label})"))? {
        ui::ok(&format!("uploaded ssh {label} key"));
    } else {
        ui::warn(&format!("ssh {label} key upload failed"));
    }
    Ok(())
}

fn add_gpg(ctx: &Ctx) -> Result<()> {
    if !signing::gpg_key_present(GPG_KEY) {
        ui::skip(&format!("gpg key {GPG_KEY} not in this keyring"));
        return Ok(());
    }
    if let Ok(listed) = Cmd::new("gh").args(["gpg-key", "list"]).capture() {
        if listed.ok && listed.stdout.contains(GPG_KEY) {
            ui::skip("gpg key already on github");
            return Ok(());
        }
    }
    if !ensure_scope(ctx, "write:gpg_key") {
        ui::warn("skipping gpg key upload");
        return Ok(());
    }
    if !ui::confirm(
        &format!("Upload gpg public key {GPG_KEY} to github?"),
        ctx.assume_yes,
    ) {
        ui::skip("declined");
        return Ok(());
    }
    let armored = Cmd::new("gpg")
        .args(["--armor", "--export", GPG_KEY])
        .capture()?;
    if !armored.ok {
        ui::warn("gpg export failed");
        return Ok(());
    }
    // Public key, so not secret -- but a predictable path is still a clobber
    // vector, and fs::write would follow a pre-planted symlink.
    let dir = PrivateTempDir::new("gpg-pub")?;
    let tmp = dir.join("key.asc");
    std::fs::write(&tmp, armored.stdout)?;
    if Cmd::new("gh")
        .args(["gpg-key", "add"])
        .arg(tmp.to_string_lossy())
        .run(ctx, "gh gpg-key add")?
    {
        ui::ok("uploaded gpg public key");
    } else {
        ui::warn("gpg key upload failed (already present?)");
    }
    Ok(())
}

fn verify(ctx: &Ctx) {
    ui::head("Verifying");

    let probe = Cmd::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "-T",
            "git@github.com",
        ])
        .capture()
        .map(|c| c.combined())
        .unwrap_or_default();
    if probe.contains("successfully authenticated") {
        let who = probe
            .lines()
            .find_map(|l| l.strip_prefix("Hi "))
            .and_then(|l| l.split('!').next())
            .unwrap_or("?");
        ui::ok(&format!("ssh: {who} authenticated"));
    } else if probe.contains("Permission denied") {
        ui::error("ssh: github rejected the key");
    } else {
        ui::warn(&format!(
            "ssh: inconclusive — {}",
            probe.lines().next().unwrap_or("")
        ));
    }

    // A real signed commit in a throwaway repo is the only honest check that
    // signing is wired up end to end.
    // A predictable path here is worse than it looks: create_dir_all succeeds
    // through a symlink to a directory, so `git init` + `git commit` could
    // land a junk commit in a real repository.
    let Ok(probe) = PrivateTempDir::new("signprobe") else {
        ui::warn("could not create a temporary directory for the signing probe");
        return;
    };
    let tmp = probe.join("repo");
    let _ = std::fs::create_dir(&tmp);
    let git = |args: &[&str]| {
        Cmd::new("git")
            .args(["-C", &tmp.to_string_lossy()])
            .args(args.iter().copied())
            .capture()
    };
    let _ = git(&["init", "-q"]);
    let _ = git(&["commit", "-q", "--allow-empty", "-m", "signing probe"]);
    let sig = git(&["log", "-1", "--format=%G?"])
        .map(|c| c.trimmed().to_string())
        .unwrap_or_default();
    match sig.as_str() {
        "G" => {
            let signer = git(&["log", "-1", "--format=%GS"])
                .map(|c| c.trimmed().to_string())
                .unwrap_or_default();
            ui::ok(&format!(
                "commit signing works and verifies locally ({signer})"
            ));
        }
        "U" => ui::warn("commits are signed but the key is untrusted locally"),
        "N" => ui::warn("commits are NOT signed — run: dotfiles install"),
        other => ui::warn(&format!("could not determine signing status ({other})")),
    }
    let _ = ctx;
    println!();
}
