//! Quality-of-life applications.

use crate::ctx::Ctx;
use crate::layer::Layer;
use crate::{brew, ui};
use anyhow::Result;

pub fn install(ctx: &Ctx, layer: &Layer) -> Result<()> {
    brew::bundle(ctx, &layer.path(ctx).join("Brewfile"))?;
    ui::detail(
        "Not on Homebrew. Install by hand if you want them:\n  \
         App Store: LastPass, Asana, Affinity\n  \
         Vendor:    NordLayer (nordlayer.com), FileZilla (filezilla-project.org)",
    );
    Ok(())
}
