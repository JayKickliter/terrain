#![deny(missing_docs)]
#![cfg_attr(not(doctest), doc = include_str!("../README.md"))]

pub use crate::{
    error::NasademError,
    sample::Sample,
    tile::{Tile, TileIndex},
};
pub use geo;
#[cfg(feature = "image")]
pub use image;

mod error;
mod sample;
pub(crate) mod store;
#[cfg(test)]
mod tests;
mod tile;
#[cfg(feature = "image")]
mod to_image;
pub(crate) mod util;

/// Base floating point type used for all coordinates and calculations.
///
/// Note: this _could_ be a generic parameter, but doing so makes the
/// library more complicated. While f32 vs f64 does make a measurable
/// difference when walking paths across tiles (see `Profile` type in
/// the `terrain` crate), benchmarking shows that switching NASADEMs
/// to `f32` has no effect.
pub type C = f64;

/// Bit representation of elevation samples.
pub type Elev = i16;

const ARCSEC_PER_DEG: C = 3600.0;
const HALF_ARCSEC: C = 1.0 / (2.0 * 3600.0);
