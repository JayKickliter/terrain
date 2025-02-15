use h3o::{LatLng, Resolution};
use hextree::disktree::DiskTreeMap;
use hextree::Cell;
use nalgebra::DMatrix;
use nasadem::{Sample, Tile};

pub fn tile_to_worldcover_matrix(h3db: &DiskTreeMap, tile: &Tile) -> DMatrix<WorldCover> {
    let (w, h) = tile.dimensions();
    let lookup_fn = |sample: Sample| {
        let geo = sample.geo();
        let cell = LatLng::new(geo.y, geo.x)
            .unwrap()
            .to_cell(Resolution::Fifteen);
        let cell = Cell::from_raw(u64::from(cell)).unwrap();
        match h3db.get(cell).unwrap() {
            None => {
                print!(".");
                WorldCover::Water
            }
            Some((_, raw)) => match WorldCover::try_from(raw[0]) {
                Ok(wc) => wc,
                _ => WorldCover::Water,
            },
        }
    };
    DMatrix::from_row_iterator(h, w, tile.iter().map(lookup_fn))
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
