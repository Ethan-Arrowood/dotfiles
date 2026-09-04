//! Work layer: Harper dev-server aliases plus the work password manager.
//!
//! No git identity override on purpose -- ethan@arrowood.dev is the identity
//! everywhere, including work repos. A domain you own outlives every employer,
//! and an address you lose control of takes its commit attribution with it.

use crate::ctx::Ctx;
use crate::layer::Layer;
use crate::sys;
use crate::{brew, ui};
use anyhow::Result;

pub fn install(ctx: &Ctx, layer: &Layer) -> Result<()> {
    let brewfile = layer.path(ctx).join("Brewfile");
    if sys::is_macos() {
        brew::bundle(ctx, &brewfile)?;
    } else {
        ui::skip("not macOS: skipping the LastPass cask");
    }
    if !ctx.home.join("dev/harperfast").is_dir() {
        ui::warn("~/dev/harperfast does not exist yet; the harper aliases assume that layout");
    }
    Ok(())
}
