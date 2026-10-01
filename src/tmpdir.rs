//! A private temporary directory for secret material.
//!
//! Downloaded installers and scratch working directories must not sit at a
//! path another user can predict or pre-create.
//! `create_dir` fails if the path exists, which makes creation atomic against a
//! pre-planted file or symlink, and 0700 means nothing inside can be reached,
//! replaced, or symlink-swapped afterwards. Files created within therefore need
//! no defence of their own -- which matters for anything that creates its
//! output at the caller's umask rather than explicitly.
//!
//! The directory removes itself on drop, including on error paths.

use anyhow::{bail, Context, Result};
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

pub struct PrivateTempDir {
    path: PathBuf,
}

impl PrivateTempDir {
    pub fn new(tag: &str) -> Result<Self> {
        let base = std::env::temp_dir();
        // A few attempts in case of an unlucky collision; create_dir is the
        // exclusivity check, so a loser simply retries.
        for attempt in 0..8u32 {
            let nonce = nonce(attempt);
            let path = base.join(format!("dotfiles-{tag}-{nonce}"));
            match std::fs::create_dir(&path) {
                Ok(()) => {
                    // Narrow the mode before anything is written into it.
                    crate::link::chmod(&path, 0o700)
                        .with_context(|| format!("securing {}", path.display()))?;
                    return Ok(Self { path });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e).with_context(|| format!("creating {}", path.display())),
            }
        }
        bail!(
            "could not create a private temporary directory in {}",
            base.display()
        )
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PrivateTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Not cryptographic — only needs to avoid collisions between concurrent runs.
/// Exclusivity comes from `create_dir`, not from unpredictability.
fn nonce(attempt: u32) -> String {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{pid}-{nanos}-{attempt}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn directory_is_private_and_self_cleaning() {
        let path = {
            let d = PrivateTempDir::new("test").unwrap();
            let mode = std::fs::metadata(d.path()).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "temp dir must not be reachable by other users");
            std::fs::write(d.join("secret"), b"x").unwrap();
            d.path().to_path_buf()
        };
        assert!(!path.exists(), "temp dir must be removed on drop");
    }

    /// The property that defeats a pre-planted file or symlink.
    #[test]
    fn creation_is_exclusive() {
        let d = PrivateTempDir::new("excl").unwrap();
        assert!(
            std::fs::create_dir(d.path()).is_err(),
            "create_dir must fail on an existing path"
        );
    }
}
