//! Terminal output and prompting.
//!
//! Every user-visible line goes through here so `--dry-run` and colour
//! handling stay consistent across layers.

use owo_colors::{OwoColorize, Stream::Stdout};
use std::io::{self, Write};

pub fn head(msg: &str) {
    println!(
        "\n{} {}",
        "==>".if_supports_color(Stdout, |t| t.blue()),
        msg.if_supports_color(Stdout, |t| t.bold())
    );
}

pub fn step(msg: &str) {
    println!("{} {}", "  ->".if_supports_color(Stdout, |t| t.blue()), msg);
}

pub fn ok(msg: &str) {
    println!(
        "{} {}",
        "   ok".if_supports_color(Stdout, |t| t.green()),
        msg
    );
}

pub fn skip(msg: &str) {
    println!(
        "{}",
        format!("  skip {msg}").if_supports_color(Stdout, |t| t.dimmed())
    );
}

pub fn dry(msg: &str) {
    println!(
        "{}",
        format!("   dry {msg}").if_supports_color(Stdout, |t| t.dimmed())
    );
}

pub fn warn(msg: &str) {
    eprintln!(
        "{} {}",
        "  warn".if_supports_color(Stdout, |t| t.yellow()),
        msg
    );
}

pub fn error(msg: &str) {
    eprintln!(
        "{} {}",
        " error".if_supports_color(Stdout, |t| t.red()),
        msg
    );
}

/// Continuation lines under a warn/ok, indented to line up.
pub fn detail(msg: &str) {
    for line in msg.lines() {
        println!("       {line}");
    }
}

/// Ask a yes/no question. Returns false when there is no terminal to ask on
/// (piped, CI, a subshell) rather than failing -- the caller treats that as
/// "not authorised", which is the safe default for anything outward-facing.
pub fn confirm(question: &str, assume_yes: bool) -> bool {
    if assume_yes {
        return true;
    }
    let Ok(mut tty) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
    else {
        println!(
            "{}",
            format!("    ? {question} -- no terminal to ask on, assuming no")
                .if_supports_color(Stdout, |t| t.dimmed())
        );
        return false;
    };
    let _ = write!(
        tty,
        "{} {question} [y/N] ",
        "    ?".if_supports_color(Stdout, |t| t.yellow())
    );
    let _ = tty.flush();

    let mut answer = String::new();
    // Read a single line straight from the terminal, not stdin, so this works
    // even when the process was invoked through a pipe.
    let mut reader = io::BufReader::new(tty);
    use std::io::BufRead;
    if reader.read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim_start().chars().next(), Some('y') | Some('Y'))
}
