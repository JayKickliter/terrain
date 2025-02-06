use h3o::{LatLng, Resolution};
use hextree::disktree::DiskTreeMap;
use hextree::Cell;
use nalgebra::DMatrix;
use nasadem::{Sample, Tile};

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
    /// HSL hue (degrees) and saturation (percent) used to tint this class.
    pub fn hue_sat(self) -> (f32, f32) {
        match self {
            WorldCover::Bare => (36.0, 92.0),
            WorldCover::Built => (209.0, 11.0),
            WorldCover::Crop => (28.0, 80.0),
            WorldCover::Frozen => (180.0, 14.0),
            WorldCover::Grass => (42.0, 46.0),
            WorldCover::Mangrove => (203.0, 51.0),
            WorldCover::Moss => (283.0, 37.0),
            WorldCover::Shrub => (54.0, 45.0),
            WorldCover::Tree => (145.0, 63.0),
            WorldCover::Water => (204.0, 64.0),
            WorldCover::Wet => (204.0, 66.0),
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
