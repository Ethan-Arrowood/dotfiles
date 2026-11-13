//! dotfiles: compose a machine from layers.

mod brew;
mod claude;
mod ctx;
mod dock;
mod doctor;
mod github;
mod hooks;
mod layer;
mod layers;
mod link;
mod ncu;
mod shellinit;
mod signing;
mod skills;
mod sys;
mod tmpdir;
mod ui;

use anyhow::{bail, Result};
use clap::{Args, Parser, Subcommand};
use ctx::Ctx;

#[derive(Parser)]
#[command(
    name = "dotfiles",
    about = "Compose a machine from layers",
    version,
    max_term_width = 100
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Install layers and link config
    Install(InstallArgs),
    /// Show available profiles and layers
    #[command(alias = "ls")]
    List,
    /// Show installed layers and links
    Status,
    /// Re-link $HOME files only, no packages
    Link,
    /// Check tools, SSH, signing and links
    Doctor,
    /// Register this machine's ssh key with GitHub (auth + signing)
    #[command(alias = "gh")]
    Github,
    /// Set up node-core-utils (ncu-ci, git-node) credentials on this machine
    Ncu,
}

#[derive(Args)]
struct InstallArgs {
    /// Profile to install (default: core, or mac-dev on macOS)
    #[arg(short, long)]
    profile: Option<String>,
    /// Add a layer to the profile
    #[arg(long, value_name = "LAYER")]
    with: Vec<String>,
    /// Drop a layer from the profile
    #[arg(long, value_name = "LAYER")]
    without: Vec<String>,
    /// Print what would happen, change nothing
    #[arg(short = 'n', long)]
    dry_run: bool,
    /// Assume yes for prompts
    #[arg(short = 'y', long)]
    yes: bool,
    /// Refresh already-installed toolchains (rustup update, uv self update)
    #[arg(long)]
    update: bool,
    /// Do not configure commit signing
    #[arg(long)]
    no_signing: bool,
}

fn main() {
    if let Err(e) = real_main() {
        ui::error(&format!("{e:#}"));
        std::process::exit(1);
    }
}

fn real_main() -> Result<()> {
    let cli = Cli::parse();

    // Only `install` overrides these defaults: run changes and prompt for consent.
    let mut ctx = Ctx::new(Ctx::find_root()?)?;

    match cli.command {
        Command::Install(args) => {
            ctx.dry_run = args.dry_run;
            ctx.assume_yes = args.yes;
            ctx.update = args.update;
            ctx.signing = !args.no_signing;
            install(&ctx, args)
        }
        Command::List => list(&ctx),
        Command::Status => status(&ctx),
        Command::Link => relink(&ctx),
        Command::Doctor => {
            if !doctor::run(&ctx)? {
                std::process::exit(1);
            }
            Ok(())
        }
        Command::Github => github::run(&ctx),
        Command::Ncu => ncu::run(&ctx),
    }
}

fn install(ctx: &Ctx, args: InstallArgs) -> Result<()> {
    let profile = match args.profile {
        Some(p) => p,
        None => {
            let p = if sys::is_macos() && !sys::is_container() {
                "mac-dev"
            } else {
                "core"
            };
            ui::step(&format!("no --profile given, defaulting to '{p}'"));
            p.to_string()
        }
    };

    let selected = layer::resolve(ctx, &profile, &args.with, &args.without)?;

    for l in &selected {
        if !l.supported() {
            bail!(
                "layer '{}' is macOS-only; this is {}",
                l.name,
                std::env::consts::OS
            );
        }
    }

    ui::head("Plan");
    println!("  profile   {profile}");
    println!(
        "  platform  {} ({})",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    println!(
        "  layers    {}",
        selected
            .iter()
            .map(|l| l.name)
            .collect::<Vec<_>>()
            .join(" ")
    );
    if ctx.dry_run {
        println!("  dry run: nothing will change");
    } else if !ui::confirm("Proceed?", ctx.assume_yes) {
        bail!("aborted");
    }

    let mut claimed = skills::Claimed::new();
    for l in &selected {
        ui::head(&format!("Layer {}: {}", l.name, l.description));
        (l.install)(ctx, l)?;
        // Every layer opts into links and Skillfiles by shipping the files.
        link::link_home_dir(ctx, &l.path(ctx))?;
        skills::install(ctx, l, &mut claimed)?;
    }
    dock::apply(ctx, &selected)?;

    ui::head("Shell init");
    shellinit::write(ctx, &profile, &selected)?;

    ui::head("Done");
    let failures = ui::warning_count();
    if failures > 0 && !ctx.dry_run {
        ui::warn(&format!(
            "{failures} warning(s); run `dotfiles doctor` to check"
        ));
    }
    if ctx.dry_run {
        println!("  dry run complete. Re-run without --dry-run to apply\n");
    } else {
        if ctx.backup_dir.exists() {
            println!(
                "  replaced files backed up to {}",
                ctx.pretty(&ctx.backup_dir)
            );
        }
        println!("  open a new shell, or: exec zsh");
        next_steps(ctx);
    }
    Ok(())
}

/// Print remaining setup steps, if any.
fn next_steps(ctx: &Ctx) {
    let mut steps: Vec<String> = Vec::new();
    let key = signing::ssh_key_path(ctx);
    let pubkey = signing::ssh_pub_key(ctx);

    if !key.exists() {
        steps.push(format!(
            "generate an ssh key:  ssh-keygen -t ed25519 -C \"{}\" -f {}",
            signing::EMAIL,
            ctx.pretty(&key)
        ));
    }

    if sys::has("gh") {
        let authed = sys::Cmd::new("gh")
            .args(["auth", "status"])
            .capture()
            .map(|c| c.ok)
            .unwrap_or(false);
        if !authed {
            steps.push("authenticate with GitHub (browser + 2FA):  gh auth login".into());
            steps.push("then register your keys:  dotfiles github".into());
        } else if let Some(pk) = &pubkey {
            let body = pk.split_whitespace().nth(1).unwrap_or_default();
            if github::registered(body, None) == Some(false) {
                steps.push("register your ssh key with GitHub:  dotfiles github".into());
            }
        }
    }

    let signing_include = ctx.state_dir.join("gitconfig.signing");
    if !signing_include.exists() {
        if key.exists() {
            steps.push(
                "commits are unsigned (--no-signing). To sign, re-run without it:  dotfiles install".into(),
            );
        } else {
            steps.push(
                "commits are unsigned. Create the ssh key above, then re-run:  dotfiles install"
                    .into(),
            );
        }
    } else if std::fs::read_to_string(&signing_include)
        .map(|t| t.contains("format = ssh"))
        .unwrap_or(false)
    {
        // GitHub needs a separate signing-key upload for Verified commits.
        if let Some(pk) = &pubkey {
            let body = pk.split_whitespace().nth(1).unwrap_or_default();
            // None (can't tell, usually the signing scope) stays quiet.
            if github::registered(body, Some("signing")) == Some(false) {
                steps.push(
                    "commits sign locally but GitHub shows Unverified until the key is\n      \
                     uploaded as a SIGNING key too:  dotfiles github"
                        .into(),
                );
            }
        }
    }

    if ncu::config_missing(ctx) {
        steps.push("set up node-core-utils credentials:  dotfiles ncu".into());
    }

    if steps.is_empty() {
        println!();
        return;
    }
    println!("\n  Next:");
    for s in steps {
        println!("    - {s}");
    }
    println!();
}

fn list(ctx: &Ctx) -> Result<()> {
    ui::head("Profiles");
    for p in layer::all_profiles(ctx)? {
        let names = layer::profile_layers(ctx, &p)?;
        println!("  {:<14} {}", p, names.join(" "));
    }
    ui::head("Layers");
    for l in layer::registry() {
        println!(
            "  {:<16} {:<14} {}",
            l.name,
            format!("[{}]", l.platform_label()),
            l.description
        );
    }
    println!();
    Ok(())
}

fn status(ctx: &Ctx) -> Result<()> {
    ui::head("Machine");
    println!(
        "  platform   {} ({}){}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        if sys::is_container() {
            ", container"
        } else {
            ""
        }
    );
    println!("  repo       {}", ctx.pretty(&ctx.root));
    match shellinit::installed(ctx) {
        Some((profile, layers)) => {
            println!("  profile    {profile}");
            println!("  layers     {}", layers.join(" "));
        }
        None => println!("  no install recorded. Run `dotfiles install`"),
    }

    ui::head("Linked files");
    for name in [
        ".zshrc",
        ".zshenv",
        ".zprofile",
        ".gitconfig",
        ".gitignore_global",
    ] {
        let p = ctx.home.join(name);
        match std::fs::read_link(&p) {
            Ok(target) if target.starts_with(&ctx.root) => {
                println!("  ok   {:<20} -> {}", ctx.pretty(&p), ctx.pretty(&target))
            }
            Ok(target) => println!(
                "  !!   {:<20} -> {} (foreign)",
                ctx.pretty(&p),
                target.display()
            ),
            Err(_) if p.exists() => {
                println!("  !!   {:<20} (regular file, not linked)", ctx.pretty(&p))
            }
            Err(_) => println!("  --   {:<20} (absent)", ctx.pretty(&p)),
        }
    }
    println!();
    Ok(())
}

fn relink(ctx: &Ctx) -> Result<()> {
    let Some((_, names)) = shellinit::installed(ctx) else {
        bail!("nothing installed yet. Run `dotfiles install` first");
    };
    ui::head("Re-linking managed files");
    let mut claimed = skills::Claimed::new();
    for n in names {
        let Some(l) = layer::find(&n) else {
            ui::warn(&format!("unknown layer in recorded state: {n}"));
            continue;
        };
        // Repair home/ and Skillfile links. SSH and app links need `install`.
        link::link_home_dir(ctx, &l.path(ctx))?;
        skills::install(ctx, &l, &mut claimed)?;
    }
    ui::detail("ssh and app configs are re-linked by `dotfiles install`");
    Ok(())
}
