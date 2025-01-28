# NASA Digital Elevation Model (NASADEM)

Load and query [NASADEM/SRTM] tiles for earth elevation data. NASADEM
is a refinement over the original SRTM dataset collected during the
STS-99 Space Suttle mission. The file format (`.hgt`) is identical
between the orignal SRTM and NASADEM datasets, and this library
handles both transparently.

[NASADEM/SRTM]: https://www.earthdata.nasa.gov/esds/competitive-programs/measures/nasadem

## Usage

```rust
use nasadem::{geo::Coord, Tile};

/// Sample tile included in this repo.
let tile_path = format!(
    "{}/../data/nasadem/1arcsecond/N38W105.hgt",
    env!("CARGO_MANIFEST_DIR")
);

let tile = Tile::load(tile_path).unwrap();

// Query the tile with a absolute geograpic
// coordinates and assert the elevation is 3772 meters.
assert_eq!(
    tile.get(Coord {
        x: -104.993_472_222_222_22,
        y: 38.790_972_222_222_22,
    }),
    Some(3772)
);

// Query the tile with a linear index and
// assert the elevation is 3772 meters.
assert_eq!(tile.get(2_707_976), Some(3772));

// Query the tile with a relative cartesian
// index and assert the elevation is 3772 meters.
assert_eq!(tile.get((24, 752)), Some(3772));
```

### Helpful Resources:

- [30-Meter SRTM Tile Downloader](https://dwtkns.com/srtm30m)
- [HGT File Layout](https://www.researchgate.net/profile/Pierre-Boulanger-4/publication/228924813/figure/fig8/AS:300852653903880@1448740270695/Description-of-a-HGT-file-structure-The-name-file-in-this-case-is-N20W100HGT.png)
- [Archive Team HGT Format](http://fileformats.archiveteam.org/index.php?title=HGT&oldid=17250)
- [SRTM Collection User Guide](https://lpdaac.usgs.gov/documents/179/SRTM_User_Guide_V3.pdf)

## Tile Layout

NASADEM height `.hgt` files use a simple, headerless binary format
with no metadata. The only variable parameter—resolution—can be
inferred from the file size.

### Example: [`N51W001.hgt`](https://e4ftl01.cr.usgs.gov/MEASURES/SRTMGL1.003/2000.02.11/N51E000.SRTMGL1.2.jpg)

For this example, a 3-arcsecond resolution tile (1201x1201) might be
named `N51W001.hgt`. In this case, the bottom-left (southwest) corner
of the tile is located at 51°N, 1°W. The the non-header values in the
following table represent the linear offset of each `i16` elevation
value in the flat memory array.

|                  |               Col 0 |   Col 1 |   Col 2 |   Col 3 | ... |           Col 1200 |
|-----------------:|--------------------:|--------:|--------:|--------:|----:|-------------------:|
|        **Row 0** |       (52°N, 1°W) 0 |       1 |       2 |       3 | ... |    (52°N, 0°) 1200 |
|        **Row 1** |                1201 |    1202 |    1203 |    1204 | ... |               2400 |
|        **Row 2** |                2401 |    2402 |    2403 |    2404 | ... |               3600 |
|        **Row 3** |                3601 |    3602 |    3603 |    3604 | ... |               4800 |
|          **...** |                 ... |     ... |     ... |     ... | ... |                ... |
|     **Row 1200** | (51°N, 1°W) 1441200 | 1441201 | 1441202 | 1441203 | ... | (51°N, 0°) 1442400 |

Note that while NASADEM filenames correspond to the southwest corner of a
tile, the first elevation value in a `.hgt` file is the northwest corner.

## License

This project is licensed under one of the following licenses:

- Apache License 2.0 ([LICENSE-APACHE](../LICENSE-APACHE) or [Apache License 2.0](http://www.apache.org/licenses/LICENSE-2.0))
- MIT License ([LICENSE-MIT](../LICENSE-MIT) or [MIT License](http://opensource.org/licenses/MIT))

## Contributions

By default, contributions you make to this project are considered
dual-licensed under both the [Apache 2.0
License](http://www.apache.org/licenses/LICENSE-2.0) and the MIT
License, with no additional conditions.
