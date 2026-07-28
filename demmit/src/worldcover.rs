//! ESA `WorldCover` land-cover classes and the colors used to tint them.

use crate::color::Rgb8;
use h3o::{LatLng, Resolution};
use hextree::disktree::DiskTreeMap;
use hextree::Cell;
use nalgebra::DMatrix;
use nasadem::{Sample, Tile};
use serde::{Deserialize, Serialize};

pub fn tile_to_worldcover_matrix(h3db: &DiskTreeMap, tile: &Tile) -> DMatrix<WorldCover> {
    let (w, h) = tile.dimensions();
    let lookup_fn = |sample: Sample| {
        let geo = sample.geo();
        worldcover_at(h3db, geo.y, geo.x)
    };
    DMatrix::from_row_iterator(h, w, tile.iter().map(lookup_fn))
}

/// Land-cover classes sampled at a reduced display resolution.
///
/// Row-major from the NW corner, matching [`Gradients::from_tile_downsampled`][crate::Gradients::from_tile_downsampled].
/// `out_px` is clamped to the tile's native resolution.
pub fn tile_to_worldcover_downsampled(
    h3db: &DiskTreeMap,
    tile: &Tile,
    lon_sw: i32,
    lat_sw: i32,
    out_px: usize,
) -> Vec<WorldCover> {
    let (nw, nh) = tile.dimensions();
    let px = out_px.clamp(2, nw.min(nh));
    let denom = (px - 1).max(1);
    let step = f64::from(tile.resolution()) / 3600.0;
    let mut out = vec![WorldCover::Water; px * px];
    for oy in 0..px {
        let ny = oy * (nh - 1) / denom;
        let lat = f64::from(lat_sw + 1) - ny as f64 * step;
        for ox in 0..px {
            let nx = ox * (nw - 1) / denom;
            let lon = f64::from(lon_sw) + nx as f64 * step;
            out[oy * px + ox] = worldcover_at(h3db, lat, lon);
        }
    }
    out
}

/// Looks up the land-cover class at a geographic point.
///
/// Returns [`WorldCover::Water`] for points with no coverage.
pub fn worldcover_at(h3db: &DiskTreeMap, lat: f64, lon: f64) -> WorldCover {
    let cell = LatLng::new(lat, lon)
        .expect("lat/lon in range")
        .to_cell(Resolution::Fifteen);
    let cell = Cell::from_raw(u64::from(cell)).expect("valid h3 cell");
    match h3db.get(cell) {
        Ok(Some((_, raw))) => WorldCover::try_from(raw[0]).unwrap_or(WorldCover::Water),
        _ => WorldCover::Water,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum WorldCover {
    Tree = 10,
    Shrub = 20,
    Grass = 30,
    Crop = 40,
    Built = 50,
    Bare = 60,
    Frozen = 70,
    Water = 80,
    Wet = 90,
    Mangrove = 95,
    Moss = 100,
}

impl std::fmt::Display for WorldCover {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.to_str())
    }
}

impl WorldCover {
    /// Every class, in [`Palette`] index order.
    pub const ALL: [WorldCover; 11] = [
        WorldCover::Bare,
        WorldCover::Built,
        WorldCover::Crop,
        WorldCover::Frozen,
        WorldCover::Grass,
        WorldCover::Mangrove,
        WorldCover::Moss,
        WorldCover::Shrub,
        WorldCover::Tree,
        WorldCover::Water,
        WorldCover::Wet,
    ];

    /// Position of this class in [`WorldCover::ALL`].
    #[inline]
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            WorldCover::Bare => 0,
            WorldCover::Built => 1,
            WorldCover::Crop => 2,
            WorldCover::Frozen => 3,
            WorldCover::Grass => 4,
            WorldCover::Mangrove => 5,
            WorldCover::Moss => 6,
            WorldCover::Shrub => 7,
            WorldCover::Tree => 8,
            WorldCover::Water => 9,
            WorldCover::Wet => 10,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn to_str(self) -> &'static str {
        match self {
            WorldCover::Tree => "TreeCover",
            WorldCover::Shrub => "Shrubland",
            WorldCover::Grass => "Grassland",
            WorldCover::Crop => "Cropland",
            WorldCover::Built => "BuiltUp",
            WorldCover::Bare => "BareOrSparseVeg",
            WorldCover::Frozen => "SnowAndIce",
            WorldCover::Water => "Water",
            WorldCover::Wet => "HerbaceousWetland",
            WorldCover::Mangrove => "Mangroves",
            WorldCover::Moss => "MossAndLichen",
        }
    }
}

/// Per-class tint colors for land-cover shading.
///
/// Only the hue and saturation of each color reach the render: the
/// hillshade supplies lightness per pixel.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(from = "PaletteToml", into = "PaletteToml")]
pub struct Palette([Rgb8; WorldCover::ALL.len()]);

impl Palette {
    /// Tint color for a class.
    #[inline]
    #[must_use]
    pub const fn get(&self, class: WorldCover) -> Rgb8 {
        self.0[class.index()]
    }

    /// Replaces the tint color for a class.
    pub fn set(&mut self, class: WorldCover, color: Rgb8) {
        self.0[class.index()] = color;
    }

    /// Hue (degrees) and saturation (percent) per class, indexed by [`WorldCover::index`].
    #[must_use]
    pub fn hue_sat_table(&self) -> [(f32, f32); WorldCover::ALL.len()] {
        self.0.map(Rgb8::hue_sat)
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::from(PaletteToml::default())
    }
}

/// Named-field mirror of [`Palette`], giving `demmit.toml` readable keys.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(default)]
struct PaletteToml {
    bare: Rgb8,
    built: Rgb8,
    crop: Rgb8,
    frozen: Rgb8,
    grass: Rgb8,
    mangrove: Rgb8,
    moss: Rgb8,
    shrub: Rgb8,
    tree: Rgb8,
    water: Rgb8,
    wet: Rgb8,
}

impl Default for PaletteToml {
    fn default() -> Self {
        let hsl = |h: f32, s: f32| Rgb8::from_hsl(h, s, 50.0);
        Self {
            bare: hsl(36.0, 92.0),
            built: hsl(209.0, 11.0),
            crop: hsl(28.0, 80.0),
            frozen: hsl(180.0, 14.0),
            grass: hsl(42.0, 46.0),
            mangrove: hsl(203.0, 51.0),
            moss: hsl(283.0, 37.0),
            shrub: hsl(54.0, 45.0),
            tree: hsl(145.0, 63.0),
            water: hsl(204.0, 64.0),
            wet: hsl(204.0, 66.0),
        }
    }
}

impl From<PaletteToml> for Palette {
    fn from(t: PaletteToml) -> Self {
        Self([
            t.bare, t.built, t.crop, t.frozen, t.grass, t.mangrove, t.moss, t.shrub, t.tree,
            t.water, t.wet,
        ])
    }
}

impl From<Palette> for PaletteToml {
    fn from(p: Palette) -> Self {
        Self {
            bare: p.get(WorldCover::Bare),
            built: p.get(WorldCover::Built),
            crop: p.get(WorldCover::Crop),
            frozen: p.get(WorldCover::Frozen),
            grass: p.get(WorldCover::Grass),
            mangrove: p.get(WorldCover::Mangrove),
            moss: p.get(WorldCover::Moss),
            shrub: p.get(WorldCover::Shrub),
            tree: p.get(WorldCover::Tree),
            water: p.get(WorldCover::Water),
            wet: p.get(WorldCover::Wet),
        }
    }
}

impl TryFrom<u8> for WorldCover {
    type Error = u8;
    fn try_from(other: u8) -> Result<WorldCover, u8> {
        let val = match other {
            10 => WorldCover::Tree,
            20 => WorldCover::Shrub,
            30 => WorldCover::Grass,
            40 => WorldCover::Crop,
            50 => WorldCover::Built,
            60 => WorldCover::Bare,
            70 => WorldCover::Frozen,
            80 => WorldCover::Water,
            90 => WorldCover::Wet,
            95 => WorldCover::Mangrove,
            100 => WorldCover::Moss,
            _ => return Err(other),
        };
        Ok(val)
    }
}

#[cfg(test)]
mod tests {
    use super::{Palette, WorldCover};
    use crate::color::Rgb8;

    #[test]
    fn all_indices_are_unique_and_dense() {
        let mut seen = [false; WorldCover::ALL.len()];
        for (i, class) in WorldCover::ALL.iter().enumerate() {
            assert_eq!(class.index(), i, "{class} out of order");
            seen[class.index()] = true;
        }
        assert!(seen.iter().all(|&s| s));
    }

    #[test]
    fn set_then_get_round_trips() {
        let mut palette = Palette::default();
        let color = Rgb8::new(1, 2, 3);
        palette.set(WorldCover::Moss, color);
        assert_eq!(palette.get(WorldCover::Moss), color);
        assert_eq!(
            palette.get(WorldCover::Tree),
            Palette::default().get(WorldCover::Tree)
        );
    }

    #[test]
    fn hue_sat_table_matches_per_class_colors() {
        let table = Palette::default().hue_sat_table();
        for class in WorldCover::ALL {
            assert_eq!(
                table[class.index()],
                Palette::default().get(class).hue_sat()
            );
        }
    }

    #[test]
    fn round_trips_toml_by_class_name() {
        let mut palette = Palette::default();
        palette.set(WorldCover::Water, Rgb8::new(0x00, 0x11, 0x22));
        let text = toml::to_string_pretty(&palette).unwrap();
        assert!(text.contains("water = \"#001122\""), "{text}");
        let back: Palette = toml::from_str(&text).unwrap();
        assert_eq!(back, palette);
    }

    /// Missing keys fall back to defaults rather than failing the parse.
    #[test]
    fn partial_toml_fills_defaults() {
        let back: Palette = toml::from_str("tree = \"#010203\"").unwrap();
        assert_eq!(back.get(WorldCover::Tree), Rgb8::new(1, 2, 3));
        assert_eq!(
            back.get(WorldCover::Water),
            Palette::default().get(WorldCover::Water)
        );
    }
}
