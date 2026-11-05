//! Shared terminal output and prompts.

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

use std::sync::atomic::{AtomicUsize, Ordering};
static WARNINGS: AtomicUsize = AtomicUsize::new(0);

/// Count warnings for the install summary, even when installation exits 0.
pub fn warning_count() -> usize {
    WARNINGS.load(Ordering::Relaxed)
}

pub fn warn(msg: &str) {
    WARNINGS.fetch_add(1, Ordering::Relaxed);
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

/// Ask for consent. Without a terminal, return false unless assume_yes is set.
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
            format!("    ? {question} (no terminal to ask on, assuming no)")
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
    // Read /dev/tty so prompts work when stdin is a pipe.
    let mut reader = io::BufReader::new(tty);
    use std::io::BufRead;
    if reader.read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim_start().chars().next(), Some('y') | Some('Y'))
}
