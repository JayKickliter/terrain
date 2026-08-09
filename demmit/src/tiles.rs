//! Tile identity: integer-degree SW corner plus resolution.

use camino::{Utf8Path, Utf8PathBuf};

/// Identifies a 1-degree tile by its SW corner and resolution.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TileKey {
    /// Integer latitude of the SW corner.
    pub lat: i32,
    /// Integer longitude of the SW corner.
    pub lon: i32,
    /// Arcseconds per sample (1 or 3).
    pub res: u8,
}

impl TileKey {
    /// Returns the NASADEM filename for this tile, e.g. `N38W106.hgt`.
    pub fn filename(self) -> String {
        let (ns, lat_abs) = if self.lat < 0 {
            ('S', -self.lat)
        } else {
            ('N', self.lat)
        };
        let (ew, lon_abs) = if self.lon < 0 {
            ('W', -self.lon)
        } else {
            ('E', self.lon)
        };
        format!("{ns}{lat_abs:02}{ew}{lon_abs:03}.hgt")
    }

    /// Joins this tile's filename onto `dir`.
    pub fn path(self, dir: &Utf8Path) -> Utf8PathBuf {
        dir.join(self.filename())
    }
}

#[cfg(test)]
mod tests {
    use super::TileKey;

    #[test]
    fn filename_quadrants() {
        assert_eq!(
            TileKey {
                lat: 38,
                lon: -106,
                res: 1
            }
            .filename(),
            "N38W106.hgt"
        );
        assert_eq!(
            TileKey {
                lat: -9,
                lon: 5,
                res: 3
            }
            .filename(),
            "S09E005.hgt"
        );
    }
}
