use std::io::Result;
use std::process::{Command, ExitStatus};

fn scutil_set(key: &str, value: &str) -> Result<ExitStatus> {
    Command::new("scutil").args(["--set", key, value]).status()
}

pub fn set_computer_name(name: &str) -> Result<ExitStatus> {
    scutil_set("ComputerName", name)
}

pub fn set_host_name(name: &str) -> Result<ExitStatus> {
    scutil_set("HostName", name)
}

pub fn set_local_host_name(name: &str) -> Result<ExitStatus> {
    scutil_set("LocalHostName", name)
}

pub fn set_net_bios_name(name: &str) -> Result<ExitStatus> {
    Command::new("defaults")
        .args([
            "write",
            "/Library/Preferences/SystemConfiguration/com.apple.smb.server",
            "NetBIOSName",
            "-string",
            &name,
        ])
        .status()
}
