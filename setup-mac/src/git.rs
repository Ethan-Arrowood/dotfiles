use std::io::Result;
use std::process::{Command, ExitStatus};

pub fn exists () -> Result<ExitStatus> {
    Command::new("git").arg("-v").status()
}

pub fn config (personal_account: bool) -> Result<()> {
    let mut git = Command::new("git");
    let git_config_global = git.args(["config", "--global"]);

    git_config_global.args(["user.name", "Ethan Arrowood"]).spawn()?;
    git_config_global.args(["user.email", if personal_account { "ethan@arrowood.dev" } else { "ethan.arrowood@vercel.com" }]).spawn()?;
    git_config_global.args(["init.defaultBranch", "main"]).spawn()?;

    Ok(())
}