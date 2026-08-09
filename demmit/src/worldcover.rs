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

/// Number of land-cover classes.
const CLASSES: usize = WorldCover::ALL.len();

/// Default tint colors, in [`WorldCover::index`] order.
///
/// Lightness is arbitrary here since only hue and saturation are used.
fn default_colors() -> [Rgb8; CLASSES] {
    let hsl = |h: f32, s: f32| Rgb8::from_hsl(h, s, 50.0);
    [
        hsl(36.0, 92.0),
        hsl(209.0, 11.0),
        hsl(28.0, 80.0),
        hsl(180.0, 14.0),
        hsl(42.0, 46.0),
        hsl(203.0, 51.0),
        hsl(283.0, 37.0),
        hsl(54.0, 45.0),
        hsl(145.0, 63.0),
        hsl(204.0, 64.0),
        hsl(204.0, 66.0),
    ]
}

/// Per-class tint colors for land-cover shading.
///
/// Only the hue and saturation of each color reach the render: the
/// hillshade supplies lightness per pixel.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(from = "ByClass<Rgb8>", into = "ByClass<Rgb8>")]
pub struct Palette([Rgb8; CLASSES]);

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
    ///
    /// Classes `mask` disables get zero saturation, which shades them
    /// as plain grayscale hillshade.
    #[must_use]
    pub fn hue_sat_table(&self, mask: &CoverMask) -> [(f32, f32); CLASSES] {
        std::array::from_fn(|i| {
            if mask.0[i] {
                self.0[i].hue_sat()
            } else {
                (0.0, 0.0)
            }
        })
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self(default_colors())
    }
}

/// Which land-cover classes are tinted, the rest shading as grayscale.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(from = "ByClass<bool>", into = "ByClass<bool>")]
pub struct CoverMask([bool; CLASSES]);

impl CoverMask {
    /// Every class tinted.
    pub const ALL_ON: Self = Self([true; CLASSES]);

    /// Every class shaded as grayscale.
    pub const ALL_OFF: Self = Self([false; CLASSES]);

    /// Whether a class is tinted.
    #[inline]
    #[must_use]
    pub const fn get(&self, class: WorldCover) -> bool {
        self.0[class.index()]
    }

    /// Sets whether a class is tinted.
    pub fn set(&mut self, class: WorldCover, tinted: bool) {
        self.0[class.index()] = tinted;
    }
}

impl Default for CoverMask {
    fn default() -> Self {
        Self::ALL_ON
    }
}

/// Per-class values in `demmit.toml`, one readable key per class.
///
/// Absent keys stay `None` so [`ByClass::resolve`] can fill them from a
/// caller-supplied default.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(default)]
struct ByClass<T> {
    bare: Option<T>,
    built: Option<T>,
    crop: Option<T>,
    frozen: Option<T>,
    grass: Option<T>,
    mangrove: Option<T>,
    moss: Option<T>,
    shrub: Option<T>,
    tree: Option<T>,
    water: Option<T>,
    wet: Option<T>,
}

impl<T> Default for ByClass<T> {
    fn default() -> Self {
        Self {
            bare: None,
            built: None,
            crop: None,
            frozen: None,
            grass: None,
            mangrove: None,
            moss: None,
            shrub: None,
            tree: None,
            water: None,
            wet: None,
        }
    }
}

impl<T: Copy> ByClass<T> {
    /// Fills absent keys from `fallback`, in [`WorldCover::index`] order.
    fn resolve(self, fallback: [T; CLASSES]) -> [T; CLASSES] {
        [
            self.bare.unwrap_or(fallback[0]),
            self.built.unwrap_or(fallback[1]),
            self.crop.unwrap_or(fallback[2]),
            self.frozen.unwrap_or(fallback[3]),
            self.grass.unwrap_or(fallback[4]),
            self.mangrove.unwrap_or(fallback[5]),
            self.moss.unwrap_or(fallback[6]),
            self.shrub.unwrap_or(fallback[7]),
            self.tree.unwrap_or(fallback[8]),
            self.water.unwrap_or(fallback[9]),
            self.wet.unwrap_or(fallback[10]),
        ]
    }
}

impl<T: Copy> From<[T; CLASSES]> for ByClass<T> {
    fn from(values: [T; CLASSES]) -> Self {
        Self {
            bare: Some(values[0]),
            built: Some(values[1]),
            crop: Some(values[2]),
            frozen: Some(values[3]),
            grass: Some(values[4]),
            mangrove: Some(values[5]),
            moss: Some(values[6]),
            shrub: Some(values[7]),
            tree: Some(values[8]),
            water: Some(values[9]),
            wet: Some(values[10]),
        }
    }
}

impl From<ByClass<Rgb8>> for Palette {
    fn from(fields: ByClass<Rgb8>) -> Self {
        Self(fields.resolve(default_colors()))
    }
}

impl From<Palette> for ByClass<Rgb8> {
    fn from(palette: Palette) -> Self {
        palette.0.into()
    }
}

impl From<ByClass<bool>> for CoverMask {
    fn from(fields: ByClass<bool>) -> Self {
        Self(fields.resolve(Self::ALL_ON.0))
    }
}

impl From<CoverMask> for ByClass<bool> {
    fn from(mask: CoverMask) -> Self {
        mask.0.into()
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
    use super::{CoverMask, Palette, WorldCover};
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
        let table = Palette::default().hue_sat_table(&CoverMask::ALL_ON);
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

    /// A disabled class must desaturate, which shades it as grayscale.
    #[test]
    fn disabled_class_loses_saturation() {
        let mut mask = CoverMask::ALL_ON;
        mask.set(WorldCover::Water, false);
        let table = Palette::default().hue_sat_table(&mask);
        assert_eq!(table[WorldCover::Water.index()], (0.0, 0.0));
        assert_ne!(table[WorldCover::Tree.index()], (0.0, 0.0));
    }

    #[test]
    fn all_off_desaturates_everything() {
        let table = Palette::default().hue_sat_table(&CoverMask::ALL_OFF);
        assert!(table.iter().all(|&hs| hs == (0.0, 0.0)));
    }

    #[test]
    fn mask_round_trips_toml_by_class_name() {
        let mut mask = CoverMask::ALL_ON;
        mask.set(WorldCover::Built, false);
        let text = toml::to_string_pretty(&mask).unwrap();
        assert!(text.contains("built = false"), "{text}");
        let back: CoverMask = toml::from_str(&text).unwrap();
        assert_eq!(back, mask);
        assert!(back.get(WorldCover::Tree));
        assert!(!back.get(WorldCover::Built));
    }

    /// Absent keys default to tinted, so old configs keep their look.
    #[test]
    fn partial_mask_toml_defaults_to_on() {
        let back: CoverMask = toml::from_str("water = false").unwrap();
        assert!(!back.get(WorldCover::Water));
        assert!(WorldCover::ALL
            .iter()
            .filter(|&&c| c != WorldCover::Water)
            .all(|&c| back.get(c)));
    }
}
