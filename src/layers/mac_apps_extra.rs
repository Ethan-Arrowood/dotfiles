//! Quality-of-life applications.

use crate::brew;
use crate::ctx::Ctx;
use crate::layer::Layer;
use anyhow::Result;

pub fn install(ctx: &Ctx, layer: &Layer) -> Result<()> {
    brew::bundle(ctx, &layer.path(ctx).join("Brewfile"))
}
