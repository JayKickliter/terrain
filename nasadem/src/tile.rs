use crate::{
    geo::{polygon, Coord, Polygon},
    store::SampleStore,
    util, Elev, NasademError, Sample, ARCSEC_PER_DEG, C, HALF_ARCSEC,
};
use memmap2::Mmap;
use std::{
    fmt,
    fs::File,
    io::BufReader,
    path::Path,
    sync::atomic::{AtomicI16, Ordering},
};

/// A NASADEM tile.
pub struct Tile {
    /// Southwest corner of the tile.
    ///
    /// Specifically, the _center_ of the SW most sample of the tile.
    sw_corner_center: Coord<C>,

    /// Northeast corner of the tile.
    ///
    /// Specifically, the _center_ of the NE most sample of the tile.
    ne_corner_center: Coord<C>,

    /// Arcseconds per sample.
    resolution: u8,

    /// Number of (rows, columns) in this tile.
    dimensions: (usize, usize),

    /// Lowest elevation sample in this tile.
    min_elevation: AtomicI16,

    /// Highest elevation sample in this tile.
    max_elevation: AtomicI16,

    /// Elevation samples.
    pub(crate) samples: SampleStore,
}

impl Tile {
    /// Returns a Tile read into memory from the file at `path`.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, NasademError> {
        let (resolution, dimensions @ (cols, rows)) = util::extract_resolution(&path)?;
        let sw_corner_center = {
            let Coord { x, y } = util::parse_sw_corner(&path)?;
            Coord {
                x: C::from(x),
                y: C::from(y),
            }
        };

        #[allow(clippy::cast_precision_loss)]
        let ne_corner_center = Coord {
            y: sw_corner_center.y + 1.0,
            x: sw_corner_center.x + 1.0,
        };

        let mut file = BufReader::new(File::open(path)?);

        let samples = {
            let mut sample_store = Vec::with_capacity(cols * rows);

            for _ in 0..(cols * rows) {
                let sample = util::read_sample(&mut file)?;
                sample_store.push(sample);
            }

            assert_eq!(sample_store.len(), dimensions.0 * dimensions.1);
            SampleStore::InMem(sample_store.into_boxed_slice())
        };

        let min_elevation = Elev::MAX.into();
        let max_elevation = Elev::MAX.into();

        Ok(Self {
            sw_corner_center,
            ne_corner_center,
            resolution,
            dimensions,
            min_elevation,
            max_elevation,
            samples,
        })
    }

    /// Returns a Tile using the memory-mapped file as storage.
    pub fn memmap<P: AsRef<Path>>(path: P) -> Result<Self, NasademError> {
        let (resolution, dimensions) = util::extract_resolution(&path)?;
        let sw_corner_center = {
            let Coord { x, y } = util::parse_sw_corner(&path)?;
            Coord {
                x: C::from(x),
                y: C::from(y),
            }
        };

        #[allow(clippy::cast_precision_loss)]
        let ne_corner_center = Coord {
            y: sw_corner_center.y as C + 1.0,
            x: sw_corner_center.x as C + 1.0,
        };

        let samples = {
            let file = File::open(path)?;
            let mmap = unsafe { Mmap::map(&file)? };
            SampleStore::MemMap(mmap)
        };

        let min_elevation = Elev::MAX.into();
        let max_elevation = Elev::MAX.into();

        Ok(Self {
            sw_corner_center,
            ne_corner_center,
            resolution,
            dimensions,
            min_elevation,
            max_elevation,
            samples,
        })
    }

    /// Returns a virtual tile that always with no elevation.
    ///
    /// A tombstone is handy when dealing with voids in SRTM coverage,
    /// e.g. oceans.
    pub fn tombstone(sw_corner: Coord<i16>, arcsec_per_sample: u8) -> Self {
        assert!(
            arcsec_per_sample == 1 || arcsec_per_sample == 3,
            "only resolution of 1 or 3 arcsecs per sample"
        );
        let sw_corner_center = Coord {
            x: C::from(sw_corner.x),
            y: C::from(sw_corner.y),
        };

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let dim = ARCSEC_PER_DEG as usize / arcsec_per_sample as usize + 1;
        let (resolution, dimensions) = (arcsec_per_sample, (dim, dim));

        #[allow(clippy::cast_precision_loss)]
        let ne_corner_center = Coord {
            y: sw_corner_center.y as C + 1.0,
            x: sw_corner_center.x as C + 1.0,
        };

        let samples = SampleStore::Tombstone(dim * dim);
        let min_elevation = 0.into();
        let max_elevation = 0.into();

        Self {
            sw_corner_center,
            ne_corner_center,
            resolution,
            dimensions,
            min_elevation,
            max_elevation,
            samples,
        }
    }

    /// Returns this tile's (x, y) dimensions.
    pub fn dimensions(&self) -> (usize, usize) {
        self.dimensions
    }

    /// Returns the number of samples in this tile.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        let (x, y) = self.dimensions();
        x * y
    }

    /// Returns the lowest elevation sample in this tile.
    pub fn min_elevation(&self) -> Elev {
        let mut min_elevation = self.min_elevation.load(Ordering::Relaxed);
        // This block can race (not data-race), but it's fine because
        // it's unlikely to happen very often if at all, and min elev
        // is min elev. The worst that can happen is the same value is
        // stored more than once, but atomically.
        if min_elevation == Elev::MAX {
            min_elevation = self.samples.min();
            self.min_elevation.store(min_elevation, Ordering::SeqCst);
        };
        min_elevation
    }

    /// Returns the highest elevation sample in this tile.
    pub fn max_elevation(&self) -> Elev {
        let mut max_elevation = self.max_elevation.load(Ordering::Relaxed);
        if max_elevation == Elev::MAX {
            // This block can race (not data-race), but it's fine because
            // it's unlikely to happen very often if at all, and max elev
            // is max elev. The worst that can happen is the same value is
            // stored more than once, but atomically.
            max_elevation = self.samples.max();
            self.max_elevation.store(max_elevation, Ordering::SeqCst);
        };
        max_elevation
    }

    /// Returns this tile's resolution in arcseconds per sample.
    pub fn resolution(&self) -> u8 {
        self.resolution
    }

    /// Returns and iterator over `self`'s grid squares.
    pub fn iter(&self) -> impl Iterator<Item = Sample<'_>> + '_ {
        (0..(self.dimensions().0 * self.dimensions().1)).map(|index| Sample { tile: self, index })
    }

    /// Returns this tile's outline as a polygon.
    pub fn polygon(&self) -> Polygon {
        let delta = C::from(self.resolution) * HALF_ARCSEC;
        let n = self.ne_corner_center.y + delta;
        let e = self.sw_corner_center.x + delta;
        let s = self.sw_corner_center.y - delta;
        let w = self.sw_corner_center.x - delta;

        polygon![
            (x: w, y: s),
            (x: e, y: s),
            (x: e, y: n),
            (x: w, y: n),
            (x: w, y: s),
        ]
    }

    /// Retrieves the elevation sample from the tile at the specified
    /// location.
    ///
    /// The `idx` parameter specifies the location to query and can be
    /// one of the following:
    ///
    /// - `usize`: The linear index of the elevation sample in the
    ///    underlying data array, where `0` corresponds to the
    ///    northwest corner of the tile.
    /// - `(usize, usize)`: A 2D index representing the `(x, y)`
    ///    position of the elevation sample, where `(0, 0)`
    ///    corresponds to the northwest corner of the tile.
    /// - `Geo`: A geographic coordinate specifying an absolute
    ///    location in latitude and longitude.
    ///
    /// # Returns
    ///
    /// - `Some(Elev)` if the location is valid and contained within
    ///    the tile.
    /// - `None` if the location is out of bounds or invalid.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use geo::Coord;
    /// use nasadem::Tile;
    ///
    /// let tile_path = format!(
    ///     "{}/../data/nasadem/1arcsecond/N38W105.hgt",
    ///     env!("CARGO_MANIFEST_DIR")
    /// );
    ///
    /// let tile = Tile::load(tile_path).unwrap();
    ///
    /// // Using a linear index.
    /// assert_eq!(tile.get(2_707_976), Some(3772));
    ///
    /// // Using relative (x, y) coordinates.
    /// assert_eq!(tile.get((24, 752)), Some(3772));
    ///
    /// // Using absolute geographic coordinates.
    /// assert_eq!(
    ///     tile.get(Coord {
    ///         x: -104.993_472_222_222_22,
    ///         y: 38.790_972_222_222_22,
    ///     }),
    ///     Some(3772)
    /// );
    /// ```
    pub fn get<T>(&self, loc: T) -> Option<Elev>
    where
        TileIndex: From<T>,
    {
        let idx = TileIndex::from(loc);
        match idx {
            TileIndex::Linear(idx) => {
                if idx < self.len() {
                    Some(self.samples.get_linear_unchecked(idx))
                } else {
                    None
                }
            }
            TileIndex::XY(idx) => self.get_xy(idx),
            TileIndex::Geo(idx) => self.get_geo(idx),
        }
    }

    /// Retrieves the elevation sample from the tile at the specified
    /// location.
    ///
    /// The `idx` parameter specifies the location to query and can be
    /// one of the following:
    ///
    /// - `usize`: The linear index of the elevation sample in the
    ///    underlying data array, where `0` corresponds to the
    ///    northwest corner of the tile.
    /// - `(usize, usize)`: A 2D index representing the `(x, y)`
    ///    position of the elevation sample, where `(0, 0)`
    ///    corresponds to the northwest corner of the tile.
    /// - `Geo`: A geographic coordinate specifying an absolute
    ///    location in latitude and longitude.
    ///
    /// # Panics
    ///
    /// This method relies raw slice indexing. It is the caller's
    /// responsibility to ensure that the location is valid and within
    /// the bounds of the tile. Passing an invalid location will
    /// result in a panic.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use geo::Coord;
    /// use nasadem::Tile;
    ///
    /// let tile_path = format!(
    ///     "{}/../data/nasadem/1arcsecond/N38W105.hgt",
    ///     env!("CARGO_MANIFEST_DIR")
    /// );
    ///
    /// let tile = Tile::load(tile_path).unwrap();
    ///
    /// // Using a linear index.
    /// assert_eq!(tile.get_unchecked(2_707_976), 3772);
    ///
    /// // Using relative (x, y) coordinates.
    /// assert_eq!(tile.get_unchecked((24, 752)), 3772);
    ///
    /// // Using absolute geographic coordinates.
    /// assert_eq!(
    ///     tile.get_unchecked(Coord {
    ///         x: -104.993_472_222_222_22,
    ///         y: 38.790_972_222_222_22,
    ///     }),
    ///     3772
    /// );
    /// ```
    pub fn get_unchecked<T>(&self, loc: T) -> Elev
    where
        TileIndex: From<T>,
    {
        let idx = TileIndex::from(loc);
        match idx {
            TileIndex::Linear(idx) => self.samples.get_linear_unchecked(idx),
            TileIndex::XY(idx) => self.get_xy_unchecked(idx),
            TileIndex::Geo(idx) => self.get_geo_unchecked(idx),
        }
    }
}

/// Private API
impl Tile {
    /// Returns the sample at the given geo coordinates.
    pub(crate) fn get_geo(&self, coord: Coord<C>) -> Option<Elev> {
        let (idx_x, idx_y) = self.geo_to_xy(coord);
        #[allow(clippy::cast_possible_wrap)]
        if 0 <= idx_x
            && idx_x < self.dimensions().0 as isize
            && 0 <= idx_y
            && idx_y < self.dimensions().1 as isize
        {
            #[allow(clippy::cast_sign_loss)]
            let idx_1d = self.xy_to_linear((idx_x as usize, idx_y as usize));
            Some(self.samples.get_linear_unchecked(idx_1d))
        } else {
            None
        }
    }

    /// Returns the sample at the given geo coordinates.
    pub(crate) fn get_geo_unchecked(&self, coord: Coord<C>) -> Elev {
        let (idx_x, idx_y) = self.geo_to_xy(coord);
        #[allow(clippy::cast_sign_loss)]
        let idx_1d = self.xy_to_linear((idx_x as usize, idx_y as usize));
        self.samples.get_linear_unchecked(idx_1d)
    }

    /// Returns the sample at the given raster coordinates.
    pub(crate) fn get_xy(&self, (x, y): (usize, usize)) -> Option<Elev> {
        if x * y < self.len() {
            Some(self.get_xy_unchecked((x, y)))
        } else {
            None
        }
    }

    /// Returns the sample at the given raster coordinates.
    pub(crate) fn get_xy_unchecked(&self, (x, y): (usize, usize)) -> Elev {
        let idx_1d = self.xy_to_linear((x, y));
        self.samples.get_linear_unchecked(idx_1d)
    }

    pub(crate) fn geo_to_xy(&self, coord: Coord<C>) -> (isize, isize) {
        let c = ARCSEC_PER_DEG / C::from(self.resolution);
        let y = (self.sw_corner_center.y + 1.0 - coord.y) * c;
        let x = (coord.x - self.sw_corner_center.x) * c;

        #[allow(clippy::cast_possible_truncation)]
        (x.round() as isize, y.round() as isize)
    }

    pub(crate) fn xy_to_geo(&self, (x, y): (usize, usize)) -> Coord<C> {
        let c = ARCSEC_PER_DEG / C::from(self.resolution);

        #[allow(clippy::cast_precision_loss)]
        let lat = self.sw_corner_center.y + 1.0 - (y as C) / c;
        #[allow(clippy::cast_precision_loss)]
        let lon = self.sw_corner_center.x + (x as C) / c;
        Coord { x: lon, y: lat }
    }

    pub(crate) fn linear_to_xy(&self, idx: usize) -> (usize, usize) {
        let y = idx / self.dimensions().0;
        let x = idx % self.dimensions().1;
        (x, y)
    }

    pub(crate) fn xy_to_linear(&self, (x, y): (usize, usize)) -> usize {
        self.dimensions().0 * y + x
    }

    pub(crate) fn xy_to_polygon(&self, (x, y): (usize, usize)) -> Polygon<C> {
        #[allow(clippy::cast_precision_loss)]
        let center = Coord {
            x: self.sw_corner_center.x + (x as C * C::from(self.resolution)) / ARCSEC_PER_DEG,
            y: self.sw_corner_center.y + (y as C * C::from(self.resolution)) / ARCSEC_PER_DEG,
        };
        util::polygon(&center, C::from(self.resolution))
    }
}

/// Represents various ways to index into a [`Tile`].
///
/// `TileIndex` is an enum that provides different indexing mechanisms
/// for a tile. Typically, you don’t need to create a `TileIndex`
/// manually; tile indexing functions are designed to work generically
/// with any of its variants.
///
/// [`Tile`]: crate::Tile
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TileIndex {
    /// Plain old offset into the flat sample array.
    Linear(usize),
    /// Cartesean coordinates, where (0, 0) is the northwest corner.
    XY((usize, usize)),
    /// Absolute geographic coordinates.
    Geo(Coord<C>),
}

impl From<usize> for TileIndex {
    /// Converts a `usize` into a `TileIndex::Linear`.
    #[inline]
    fn from(other: usize) -> TileIndex {
        TileIndex::Linear(other)
    }
}

impl From<(usize, usize)> for TileIndex {
    /// Converts a tuple `(usize, usize)` into a `TileIndex::XY`.
    #[inline]
    fn from(other: (usize, usize)) -> TileIndex {
        TileIndex::XY(other)
    }
}

impl From<Coord> for TileIndex {
    /// Converts a `Coord` into a `TileIndex::Geo`.
    #[inline]
    fn from(other: Coord<C>) -> TileIndex {
        TileIndex::Geo(other)
    }
}

impl fmt::Debug for Tile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // force lazy evaluation of max and min elevation.
        let _ = self.max_elevation();
        let _ = self.min_elevation();
        f.debug_struct("Tile")
            .field("sw_corner_center", &self.sw_corner_center)
            .field("ne_corner_center", &self.ne_corner_center)
            .field("resolution", &self.resolution)
            .field("dimensions", &self.dimensions)
            .field("min_elev", &self.min_elevation)
            .field("max_elevation", &self.max_elevation)
            .field(
                "samples",
                &match self.samples {
                    SampleStore::Tombstone(_) => "Tombstone",
                    SampleStore::InMem(_) => "InMem",
                    SampleStore::MemMap(_) => "MemMap",
                },
            )
            .finish()
    }
}
