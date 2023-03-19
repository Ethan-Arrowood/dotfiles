use regex::Regex;
use std::fs::{remove_file, File};
use std::io::Result;
use std::process::{Command, ExitStatus};
use std::str;

pub fn exists() -> Result<ExitStatus> {
    Command::new("xcode-select").arg("-p").status()
}

pub fn install() -> Result<()> {
    let file_path = "/tmp/.com.apple.dt.CommandLineTools.installondemand.in-progress;";
    File::create(file_path)?;

    let mut softwareupdate = Command::new("softwareupdate");

    let softwareupdate_output = softwareupdate.arg("-l").output()?;

    let xcode_name = Regex::new(r"(?m)^\*.*Command Line")
        .unwrap()
        .captures_iter(str::from_utf8(&softwareupdate_output.stdout).unwrap())
        .last()
        .unwrap()
        .get(1)
        .unwrap()
        .as_str()
        .replace("* Label: ", "");

    softwareupdate
        .args(["--install", &xcode_name, "--verbose"])
        .output()?;

    remove_file(file_path)?;

    Ok(())
}
