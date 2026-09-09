//! Core layer: make zsh, git and ssh usable anywhere.
//!
//! Key material only. This says nothing about GitHub: `gh` is not installed
//! yet at this point in the layer order, so a note about registration here
//! would be noise at the wrong stage. Registration is `dotfiles github`.

use crate::ctx::Ctx;
use crate::layer::Layer;
use crate::signing::{self, EMAIL};
use crate::sys::{self, Cmd};
use crate::{link, ui};
use anyhow::Result;
use std::path::Path;

pub fn install(ctx: &Ctx, layer: &Layer) -> Result<()> {
    let dir = layer.path(ctx);
    packages(ctx)?;
    ssh_config(ctx, &dir)?;
    ssh_key(ctx)?;
    ssh_agent(ctx)?;
    signing::configure(ctx, signing::Mode::from_env())?;
    login_shell(ctx)?;
    Ok(())
}

fn packages(ctx: &Ctx) -> Result<()> {
    let mut missing = Vec::new();
    if !sys::has("zsh") {
        missing.push("zsh");
    }
    if !sys::has("git") {
        missing.push("git");
    }
    if missing.is_empty() {
        ui::skip("zsh and git already installed");
        return Ok(());
    }
    ui::step(&format!("installing: {}", missing.join(" ")));
    if !sys::sys_install(ctx, &missing)? {
        ui::warn(&format!(
            "could not install {} automatically",
            missing.join(" ")
        ));
    }
    Ok(())
}

fn ssh_config(ctx: &Ctx, layer_dir: &Path) -> Result<()> {
    let ssh_dir = ctx.home.join(".ssh");
    let config_d = ssh_dir.join("config.d");
    if !ctx.dry_run {
        std::fs::create_dir_all(&config_d)?;
        link::chmod(&ssh_dir, 0o700)?;
        link::chmod(&config_d, 0o700)?;
    }

    let src_dir = layer_dir.join("ssh");
    if src_dir.is_dir() {
        let mut files: Vec<_> = std::fs::read_dir(&src_dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "conf").unwrap_or(false))
            .collect();
        files.sort();
        for f in files {
            let name = f.file_name().unwrap().to_owned();
            link::link_file(ctx, &f, &config_d.join(name))?;
        }
    }

    // ssh applies the FIRST value it sees for a keyword, so the Include has to
    // come before anything already in the file for these settings to win.
    let cfg = ssh_dir.join("config");
    const INCLUDE: &str = "Include ~/.ssh/config.d/*";

    // `.ok()` here would map EVERY error to "absent" and then truncate: an
    // unreadable, EISDIR, or dangling-symlink ~/.ssh/config would be replaced
    // by a single Include line, with no backup, since the backup only happens
    // on the Some() arm. Only NotFound means there is nothing there.
    let existing = match std::fs::read_to_string(&cfg) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            ui::error(&format!(
                "cannot read {}: {e}. Leaving it alone rather than replacing it.",
                ctx.pretty(&cfg)
            ));
            ui::detail(&format!(
                "the managed fragments are already in {}; add this line yourself:\n  {INCLUDE}",
                ctx.pretty(&config_d)
            ));
            return Ok(());
        }
    };
    match existing {
        None => {
            link::write_file(ctx, &cfg, &format!("{INCLUDE}\n"))?;
            if !ctx.dry_run {
                link::chmod(&cfg, 0o600)?;
            }
        }
        Some(text) if text.contains("config.d/") => {
            ui::skip("~/.ssh/config already includes config.d/");
        }
        Some(text) => {
            if ctx.dry_run {
                ui::dry(&format!("prepend `{INCLUDE}` to ~/.ssh/config"));
                return Ok(());
            }
            std::fs::create_dir_all(&ctx.backup_dir)?;
            std::fs::write(ctx.backup_dir.join("ssh_config"), &text)?;
            // If ~/.ssh/config is itself a symlink, fs::write follows it and
            // edits the target -- which may well be a file tracked in this
            // repo. Replace the link instead.
            if std::fs::symlink_metadata(&cfg)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
            {
                std::fs::remove_file(&cfg)?;
            }
            std::fs::write(&cfg, format!("{INCLUDE}\n\n{text}"))?;
            link::chmod(&cfg, 0o600)?;
            ui::ok(&format!(
                "prepended Include to ~/.ssh/config (backup in {})",
                ctx.pretty(&ctx.backup_dir)
            ));
            if text.lines().any(|l| {
                let l = l.trim();
                l.starts_with("Host ") && l.contains("github.com")
            }) {
                ui::warn(
                    "your existing ~/.ssh/config also has a 'Host github.com' block; \
                     the managed one now wins and yours is dead config",
                );
            }
        }
    }
    Ok(())
}

fn ssh_key(ctx: &Ctx) -> Result<()> {
    let key = signing::ssh_key_path(ctx);
    if key.exists() {
        ui::skip(&format!("ssh key {}", ctx.pretty(&key)));
        return Ok(());
    }
    if ctx.dry_run {
        ui::dry(&format!("generate ssh key {}", ctx.pretty(&key)));
        return Ok(());
    }
    // Containers borrow the host key over a forwarded agent, so they need none.
    if sys::is_container() {
        ui::skip("container: using the forwarded agent rather than a local key");
        return Ok(());
    }
    // -y means "don't ask me yes/no questions"; a passphrase is input, not a
    // question, so -y is not a reason to skip a key the machine needs. But a
    // machine with no terminal cannot be asked for one.
    if !Path::new("/dev/tty").exists() || std::fs::File::open("/dev/tty").is_err() {
        ui::warn(&format!(
            "no ssh key at {} and no terminal to set a passphrase on:",
            ctx.pretty(&key)
        ));
        ui::detail(&format!(
            "ssh-keygen -t ed25519 -C \"{EMAIL}\" -f {}",
            ctx.pretty(&key)
        ));
        return Ok(());
    }

    std::fs::create_dir_all(ctx.home.join(".ssh"))?;
    ui::step("generating an ed25519 ssh key (choose a passphrase when prompted)");
    let ok = Cmd::new("ssh-keygen")
        .args(["-t", "ed25519", "-C", EMAIL, "-f"])
        .arg(key.to_string_lossy())
        .run(ctx, "ssh-keygen")?;
    if !ok {
        ui::warn("ssh-keygen failed");
    }
    Ok(())
}

fn ssh_agent(ctx: &Ctx) -> Result<()> {
    let key = signing::ssh_key_path(ctx);
    if !key.exists() || ctx.dry_run {
        return Ok(());
    }
    let fingerprint = Cmd::new("ssh-keygen")
        .arg("-lf")
        .arg(key.to_string_lossy())
        .capture()
        .ok()
        .filter(|c| c.ok)
        .and_then(|c| c.stdout.split_whitespace().nth(1).map(String::from));

    if let Some(fp) = &fingerprint {
        if let Ok(listed) = Cmd::new("ssh-add").arg("-l").capture() {
            if listed.stdout.contains(fp.as_str()) {
                ui::skip("ssh key already loaded in the agent");
                return Ok(());
            }
        }
    }

    // --apple-use-keychain stores the passphrase so this survives reboots.
    let cmd = if sys::is_macos() {
        Cmd::new("ssh-add")
            .arg("--apple-use-keychain")
            .arg(key.to_string_lossy())
    } else {
        Cmd::new("ssh-add").arg(key.to_string_lossy())
    };
    if !cmd.run(ctx, "adding key to ssh-agent")? {
        ui::warn("ssh-add failed (is the agent running?)");
    }
    Ok(())
}

fn login_shell(ctx: &Ctx) -> Result<()> {
    let Some(zsh) = sys::which("zsh") else {
        return Ok(());
    };
    let current = std::env::var("SHELL").unwrap_or_default();
    if current == zsh.to_string_lossy() {
        ui::skip("login shell is already zsh");
        return Ok(());
    }
    if sys::is_container() {
        ui::skip("container: leaving login shell alone");
        return Ok(());
    }
    let shells = std::fs::read_to_string("/etc/shells").unwrap_or_default();
    if !shells.lines().any(|l| l.trim() == zsh.to_string_lossy()) {
        ui::warn(&format!(
            "{} is not in /etc/shells; add it before chsh",
            zsh.display()
        ));
        return Ok(());
    }
    if !ui::confirm(
        &format!("Change login shell to {}?", zsh.display()),
        ctx.assume_yes,
    ) {
        ui::skip("login shell unchanged");
        return Ok(());
    }
    if !Cmd::new("chsh")
        .arg("-s")
        .arg(zsh.to_string_lossy())
        .run(ctx, "chsh")?
    {
        ui::warn("chsh failed; run it yourself");
    }
    Ok(())
}
