//! Register the same SSH key as authentication and signing keys.
//! Authentication needs `admin:public_key`; signing needs `admin:ssh_signing_key`.
//! Also move HTTPS checkouts to ssh remotes once ssh to GitHub works.

use crate::ctx::Ctx;
use crate::signing;
use crate::sys::{self, Cmd};
use crate::tmpdir::PrivateTempDir;
use crate::ui;
use anyhow::{bail, Result};
use std::path::Path;

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

/// Check registration by key type. `None` means unknown, not absent.
/// Without the signing scope, gh lists auth keys and exits 0 with a stderr warning.
pub(crate) fn registered(pubkey_body: &str, kind: Option<&str>) -> Option<bool> {
    let out = Cmd::new("gh").args(["ssh-key", "list"]).capture().ok()?;
    if !out.ok {
        return None;
    }
    let kind = kind.unwrap_or("authentication");
    if kind == "signing" && out.stderr.contains("ssh_signing_key") {
        return None;
    }
    Some(listed(&out.stdout, pubkey_body, kind))
}

fn listed(stdout: &str, pubkey_body: &str, kind: &str) -> bool {
    stdout
        .lines()
        .any(|l| l.contains(pubkey_body) && l.rsplit('\t').next() == Some(kind))
}

pub fn run(ctx: &Ctx) -> Result<()> {
    ui::head("GitHub key setup");
    // Inherited gh credentials can reach the real account even with a sandboxed $HOME.
    if !sys::allow_global_changes() {
        return Ok(());
    }
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
    let pub_path = signing::ssh_pub_path(ctx);
    if !pub_path.exists() {
        bail!("no ssh key at {}. Run: dotfiles install", ctx.pretty(&key));
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
        .args(["config", "--global", "--includes", "gpg.format"])
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
        ui::skip("git is not set to sign with ssh. Run `dotfiles install` first");
    }

    if verify(ctx) {
        own_checkout(ctx)?;
    }
    println!();
    Ok(())
}

/// install.sh clones this repo shallow over HTTPS, before any key exists.
/// Once ssh works, switch its origin to ssh and fetch the full history.
fn own_checkout(ctx: &Ctx) -> Result<()> {
    if !ctx.root.join(".git").exists() {
        return Ok(());
    }
    use_ssh_origin(ctx, &ctx.root, None, || true)?;
    unshallow(ctx, &ctx.root)?;
    Ok(())
}

fn unshallow(ctx: &Ctx, dir: &Path) -> Result<()> {
    let git = || Cmd::new("git").args(["-C", &dir.to_string_lossy()]);
    let shallow = git()
        .args(["rev-parse", "--is-shallow-repository"])
        .capture()?;
    if !shallow.ok || shallow.trimmed() != "true" {
        return Ok(());
    }
    let desc = format!("fetching the full history of {}", ctx.pretty(dir));
    if !git()
        .args(["fetch", "--quiet", "--unshallow", "origin"])
        .run(ctx, &desc)?
    {
        ui::warn(&format!("could not unshallow {}", ctx.pretty(dir)));
    }
    Ok(())
}

/// The ssh URL for a `https://github.com/<owner>/<repo>` URL. With `slug`, only
/// when the URL names that repo. GitHub names are case-insensitive.
pub(crate) fn ssh_url_for(url: &str, slug: Option<&str>) -> Option<String> {
    let (host, path) = url.strip_prefix("https://")?.split_once('/')?;
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    if !host.eq_ignore_ascii_case("github.com") {
        return None;
    }
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, name) = path.split_once('/')?;
    if owner.is_empty() || name.is_empty() || name.contains('/') {
        return None;
    }
    if slug.is_some_and(|s| !s.eq_ignore_ascii_case(path)) {
        return None;
    }
    Some(format!("git@github.com:{path}.git"))
}

/// Point a checkout's HTTPS `origin` at the same GitHub repo over ssh, so
/// pushes use the ssh key. Wait until `ssh_ok` says ssh to GitHub works:
/// before that, an ssh origin breaks fetches. Only `remote.origin.url`
/// changes. An existing `pushurl` and other remotes stay as they are.
pub(crate) fn use_ssh_origin(
    ctx: &Ctx,
    dir: &Path,
    slug: Option<&str>,
    ssh_ok: impl FnOnce() -> bool,
) -> Result<()> {
    let git = || Cmd::new("git").args(["-C", &dir.to_string_lossy()]);
    // Read the raw value: `git remote get-url` applies insteadOf rewrites.
    let url = git()
        .args(["config", "--get", "remote.origin.url"])
        .capture()?;
    if !url.ok {
        return Ok(());
    }
    let Some(new) = ssh_url_for(url.trimmed(), slug) else {
        return Ok(());
    };
    let name = slug.map_or_else(|| ctx.pretty(dir), str::to_string);
    if !ssh_ok() {
        ui::skip(&format!(
            "{name}: origin stays on HTTPS until ssh to GitHub works (`dotfiles github`)"
        ));
        return Ok(());
    }
    if ctx.dry_run {
        ui::dry(&format!("{name}: set origin to {new}"));
        return Ok(());
    }
    let set = git()
        .args(["remote", "set-url", "origin", &new])
        .capture()?;
    if set.ok {
        ui::ok(&format!("{name}: origin now {new}"));
    } else {
        ui::warn(&format!(
            "{name}: could not set origin to {new}: {}",
            set.combined().trim()
        ));
    }
    Ok(())
}

/// Ask GitHub over ssh who this key belongs to. Batch mode never prompts.
fn ssh_probe() -> String {
    Cmd::new("ssh")
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
        .unwrap_or_default()
}

/// Whether ssh to GitHub authenticates. Probed once per run.
pub(crate) fn ssh_works() -> bool {
    static OK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OK.get_or_init(|| ssh_probe().contains("successfully authenticated"))
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
        Some("signing") => "Upload the same key as a signing key too?".to_string(),
        _ => format!("Upload {} as an authentication key?", ctx.pretty(pub_path)),
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

/// Report ssh and signing status. Return whether ssh authenticates.
fn verify(ctx: &Ctx) -> bool {
    ui::head("Verifying");

    let probe = ssh_probe();
    let ssh_ok = probe.contains("successfully authenticated");
    if ssh_ok {
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
            "ssh: inconclusive ({})",
            probe.lines().next().unwrap_or("")
        ));
    }

    // Verify signing with a temporary commit. Exclusive directory creation prevents
    // a planted symlink from directing it into a real repo.
    let Ok(probe) = PrivateTempDir::new("signprobe") else {
        ui::warn("could not create a temporary directory for the signing probe");
        return ssh_ok;
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
        "N" => ui::warn("commits are NOT signed. Run: dotfiles install"),
        other => ui::warn(&format!("could not determine signing status ({other})")),
    }
    let _ = ctx;
    ssh_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listed_matches_the_key_and_its_type() {
        let out = "Laptop\t1\tssh-ed25519 AAAA\t2026-01-01\tauthentication\n\
                   Laptop (signing)\t2\tssh-ed25519 BBBB\t2026-01-01\tsigning\n";
        assert!(listed(out, "AAAA", "authentication"));
        assert!(!listed(out, "AAAA", "signing"));
        assert!(listed(out, "BBBB", "signing"));
        assert!(!listed(out, "BBBB", "authentication"));
    }

    #[test]
    fn ssh_url_matches_the_same_repo_in_any_case() {
        let ssh = Some("git@github.com:Ethan-Arrowood/agentic.git".to_string());
        for url in [
            "https://github.com/Ethan-Arrowood/agentic.git",
            "https://github.com/Ethan-Arrowood/agentic",
            "https://github.com/Ethan-Arrowood/agentic/",
            "https://someone@GitHub.com/Ethan-Arrowood/agentic.git",
        ] {
            assert_eq!(ssh_url_for(url, None), ssh, "{url}");
            assert_eq!(
                ssh_url_for(url, Some("ethan-arrowood/AGENTIC")),
                ssh,
                "{url}"
            );
        }
        let url = "https://github.com/Ethan-Arrowood/agentic.git";
        assert_eq!(ssh_url_for(url, Some("Ethan-Arrowood/other")), None);
        for url in [
            "git@github.com:Ethan-Arrowood/agentic.git",
            "http://github.com/Ethan-Arrowood/agentic.git",
            "https://gitlab.com/Ethan-Arrowood/agentic.git",
            "https://github.com/Ethan-Arrowood",
            "https://github.com/a/b/c",
        ] {
            assert_eq!(ssh_url_for(url, None), None, "{url}");
        }
    }

    /// Run git in `dir` without the machine's identity or signing config.
    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Cmd::new("git")
            .args(["-C", &dir.to_string_lossy()])
            .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args.iter().copied())
            .capture()
            .unwrap();
        assert!(out.ok, "git {args:?}: {}", out.combined());
        out.trimmed().to_string()
    }

    fn scratch_repo(tmp: &PrivateTempDir, url: &str) -> (std::path::PathBuf, Ctx) {
        let repo = tmp.join("repo");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        git(&repo, &["remote", "add", "origin", url]);
        (repo, Ctx::new(tmp.path().to_path_buf()).unwrap())
    }

    #[test]
    fn https_origin_moves_to_ssh_once_and_keeps_pushurl() {
        let tmp = PrivateTempDir::new("sshorigin").unwrap();
        let (repo, mut ctx) = scratch_repo(&tmp, "https://github.com/Ethan-Arrowood/dotfiles.git");
        let push = "git@github.com:Ethan-Arrowood/dotfiles.git";
        git(&repo, &["config", "remote.origin.pushurl", push]);
        let url = || git(&repo, &["config", "--get", "remote.origin.url"]);
        let slug = Some("ethan-arrowood/dotfiles");

        // ssh not working yet, a dry run, or another repo: no change.
        use_ssh_origin(&ctx, &repo, slug, || false).unwrap();
        use_ssh_origin(&ctx, &repo, Some("o/other"), || true).unwrap();
        ctx.dry_run = true;
        use_ssh_origin(&ctx, &repo, slug, || true).unwrap();
        assert_eq!(url(), "https://github.com/Ethan-Arrowood/dotfiles.git");

        ctx.dry_run = false;
        use_ssh_origin(&ctx, &repo, slug, || true).unwrap();
        assert_eq!(url(), "git@github.com:Ethan-Arrowood/dotfiles.git");
        assert_eq!(
            git(&repo, &["config", "--get", "remote.origin.pushurl"]),
            push
        );

        // A second run finds nothing to do and does not probe ssh.
        use_ssh_origin(&ctx, &repo, slug, || panic!("probed ssh")).unwrap();
    }

    #[test]
    fn a_shallow_clone_is_unshallowed() {
        let tmp = PrivateTempDir::new("unshallow").unwrap();
        let src = tmp.join("src");
        std::fs::create_dir(&src).unwrap();
        git(&src, &["init", "-q"]);
        for m in ["one", "two"] {
            git(&src, &["commit", "-q", "--allow-empty", "-m", m]);
        }
        let shallow = tmp.join("shallow");
        let url = format!("file://{}", src.display());
        git(
            tmp.path(),
            &["clone", "-q", "--depth", "1", &url, "shallow"],
        );
        assert_eq!(
            git(&shallow, &["rev-parse", "--is-shallow-repository"]),
            "true"
        );

        let ctx = Ctx::new(tmp.path().to_path_buf()).unwrap();
        unshallow(&ctx, &shallow).unwrap();
        assert_eq!(
            git(&shallow, &["rev-parse", "--is-shallow-repository"]),
            "false"
        );
        assert_eq!(git(&shallow, &["rev-list", "--count", "HEAD"]), "2");
    }
}
