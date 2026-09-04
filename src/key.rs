//! Portable, encrypted backup of the GPG signing key.
//!
//! A bare `gpg --export-secret-keys` is protected only by the key's own
//! passphrase, so anywhere it lands has to be treated as a secret store.
//! Wrapped with age (or `gpg --symmetric`), the artifact is an opaque blob,
//! which makes it safe to keep somewhere merely convenient.
//!
//! It is still not safe to put the UNWRAPPED key in any of those places.

use crate::ctx::Ctx;
use crate::signing::{self, GPG_KEY};
use crate::sys::{self, Cmd};
use crate::ui;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cipher {
    Age,
    Gpg,
}

impl Cipher {
    fn detect() -> Option<Self> {
        if sys::has("age") {
            Some(Cipher::Age)
        } else if sys::has("gpg") {
            Some(Cipher::Gpg)
        } else {
            None
        }
    }
    fn ext(self) -> &'static str {
        match self {
            Cipher::Age => "age",
            Cipher::Gpg => "gpg",
        }
    }
    fn name(self) -> &'static str {
        self.ext()
    }
}

pub fn status(ctx: &Ctx) -> Result<()> {
    ui::head("Signing material on this machine");

    if signing::gpg_key_present(GPG_KEY) {
        ui::ok(&format!("gpg secret key {GPG_KEY} present"));
        if let Ok(c) = Cmd::new("gpg")
            .args(["--list-secret-keys", "--keyid-format=long", GPG_KEY])
            .capture()
        {
            ui::detail(c.stdout.trim());
        }
    } else {
        ui::skip(&format!("gpg secret key {GPG_KEY} not in this keyring"));
    }

    let key = signing::ssh_key_path(ctx);
    if key.exists() {
        ui::ok(&format!("ssh key {}", ctx.pretty(&key)));
        if let Ok(c) = Cmd::new("ssh-keygen")
            .arg("-lf")
            .arg(key.with_extension("pub").to_string_lossy())
            .capture()
        {
            ui::detail(c.stdout.trim());
        }
    } else {
        ui::skip(&format!("no ssh key at {}", ctx.pretty(&key)));
    }

    match Cipher::detect() {
        Some(c) => ui::ok(&format!("export/import available (using {})", c.name())),
        None => ui::warn("neither age nor gpg installed; export/import unavailable"),
    }
    println!();
    Ok(())
}

pub fn export(ctx: &Ctx, out: Option<PathBuf>) -> Result<()> {
    if !sys::has("gpg") {
        bail!("gpg is not installed");
    }
    if !signing::gpg_key_present(GPG_KEY) {
        bail!("gpg secret key {GPG_KEY} is not in this keyring — nothing to export");
    }
    let cipher = Cipher::detect().context("need age or gpg to encrypt the export")?;

    let out = out.unwrap_or_else(|| {
        let stamp = Cmd::new("date")
            .arg("+%Y%m%d")
            .capture()
            .map(|c| c.trimmed().to_string())
            .unwrap_or_else(|_| "backup".into());
        ctx.home
            .join(format!("gpg-signing-key-{stamp}.{}", cipher.ext()))
    });
    if out.exists() {
        bail!("{} already exists; pick another path", out.display());
    }

    ui::head(&format!("Exporting {GPG_KEY}"));
    ui::detail(
        "This asks for TWO passphrases:\n  \
         1. the key's own passphrase, to unlock it for export\n  \
         2. a new passphrase to encrypt the file with",
    );
    println!();

    let tmp = temp_path("gpg-export");
    let armored = Cmd::new("gpg")
        .args(["--export-secret-keys", "--armor", GPG_KEY])
        .capture()?;
    if !armored.ok || armored.stdout.trim().is_empty() {
        // A failure here is nearly always the agent: no pinentry, or no tty.
        bail!(
            "gpg export failed (is gpg-agent able to prompt? check pinentry)\n{}",
            armored.stderr.trim()
        );
    }
    write_private(&tmp, armored.stdout.as_bytes())?;

    let encrypted = match cipher {
        Cipher::Age => Cmd::new("age")
            .args(["--passphrase", "--output"])
            .arg(out.to_string_lossy())
            .arg(tmp.to_string_lossy())
            .run(ctx, "age --passphrase")?,
        Cipher::Gpg => Cmd::new("gpg")
            .args(["--symmetric", "--cipher-algo", "AES256", "--output"])
            .arg(out.to_string_lossy())
            .arg(tmp.to_string_lossy())
            .run(ctx, "gpg --symmetric")?,
    };
    let _ = std::fs::remove_file(&tmp);
    if !encrypted {
        bail!("encryption failed");
    }
    crate::link::chmod(&out, 0o600)?;

    let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
    ui::ok(&format!(
        "wrote {} ({size} bytes, {})",
        ctx.pretty(&out),
        cipher.name()
    ));
    println!();
    ui::detail(
        "Safe to store in a private repo, gist, or cloud storage: without the\n\
         passphrase it is an opaque blob. Keep that passphrase in your password\n\
         manager, NOT next to the file.",
    );
    println!();
    Ok(())
}

pub fn import_file(ctx: &Ctx, file: &Path) -> Result<()> {
    if !file.is_file() {
        bail!("no such file: {}", file.display());
    }
    if !sys::has("gpg") {
        bail!("gpg is not installed");
    }
    let cipher = if file.extension().map(|e| e == "age").unwrap_or(false) {
        if !sys::has("age") {
            bail!("{} needs age to decrypt; brew install age", file.display());
        }
        Cipher::Age
    } else {
        Cipher::Gpg
    };

    ui::head(&format!("Importing from {}", ctx.pretty(file)));
    let tmp = temp_path("gpg-import");
    let decrypted = match cipher {
        Cipher::Age => Cmd::new("age")
            .args(["--decrypt", "--output"])
            .arg(tmp.to_string_lossy())
            .arg(file.to_string_lossy())
            .run(ctx, "age --decrypt")?,
        Cipher::Gpg => Cmd::new("gpg")
            .args(["--decrypt", "--output"])
            .arg(tmp.to_string_lossy())
            .arg(file.to_string_lossy())
            .run(ctx, "gpg --decrypt")?,
    };
    if !decrypted {
        let _ = std::fs::remove_file(&tmp);
        bail!("decryption failed (wrong passphrase?)");
    }
    let result = import_armored(ctx, &tmp);
    let _ = std::fs::remove_file(&tmp);
    result
}

/// Pull the armored key straight out of the Dashlane vault, so a new machine
/// never needs a file copied onto it by hand.
pub fn import_dashlane(ctx: &Ctx, reference: &str) -> Result<()> {
    if !sys::has("dcli") {
        bail!("dcli is not installed (brew install dashlane/tap/dashlane-cli)");
    }
    if !sys::has("gpg") {
        bail!("gpg is not installed");
    }
    let reference = if reference.starts_with("dl://") {
        reference.to_string()
    } else {
        format!("dl://{reference}")
    };

    ui::head(&format!("Importing from Dashlane ({reference})"));
    ui::detail("dcli will prompt for your master password if the session is locked.");
    println!();

    let out = Cmd::new("dcli").args(["read", &reference]).capture()?;
    if !out.ok || out.stdout.trim().is_empty() {
        bail!(
            "dcli read failed — check the reference with: dcli password <title> -o json\n{}",
            out.stderr.trim()
        );
    }
    if !out.stdout.contains("PRIVATE KEY BLOCK") {
        bail!(
            "that secret is not a PGP private key (a public-key export will not restore signing)"
        );
    }

    let tmp = temp_path("gpg-dashlane");
    write_private(&tmp, out.stdout.as_bytes())?;
    let result = import_armored(ctx, &tmp);
    let _ = std::fs::remove_file(&tmp);
    result
}

fn import_armored(ctx: &Ctx, path: &Path) -> Result<()> {
    if !Cmd::new("gpg")
        .arg("--import")
        .arg(path.to_string_lossy())
        .run(ctx, "gpg --import")?
    {
        bail!("gpg import failed");
    }
    trust(ctx)?;
    ui::ok("imported");
    println!();
    ui::detail("Now re-run the installer so git picks it up:\n  dotfiles install");
    println!();
    Ok(())
}

/// An imported key carries no owner trust, and git refuses to sign with a key
/// it does not ultimately trust.
fn trust(_ctx: &Ctx) -> Result<()> {
    if !signing::gpg_key_present(GPG_KEY) {
        return Ok(());
    }
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("gpg")
        .args(["--command-fd", "0", "--edit-key", GPG_KEY])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(b"trust\n5\ny\nquit\n");
    }
    let status = child.wait()?;
    if status.success() {
        ui::ok(&format!("set ultimate owner trust on {GPG_KEY}"));
    } else {
        ui::warn(&format!(
            "could not set owner trust; run: gpg --edit-key {GPG_KEY} trust"
        ));
    }
    Ok(())
}

fn temp_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("dotfiles-{tag}-{}", std::process::id()))
}

/// Write key material 0600 from the start, never world-readable even briefly.
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("creating {}", path.display()))?;
    f.write_all(bytes)?;
    Ok(())
}
