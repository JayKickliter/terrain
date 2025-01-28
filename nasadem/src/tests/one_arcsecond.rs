use crate::{geo::Coord, util, Tile};
use std::{fs::File, io::BufReader, path::PathBuf};

fn one_arcsecond_dir() -> PathBuf {
    [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "data",
        "nasadem",
        "1arcsecond",
    ]
    .iter()
    .collect()
}

#[test]
fn test_parse_hgt_name() {
    let mut path = one_arcsecond_dir();
    path.push("N44W072.hgt");
    let sw_corner = util::parse_sw_corner(&path).unwrap();
    let resolution = util::extract_resolution(&path).unwrap();
    assert_eq!(sw_corner, Coord { x: -72, y: 44 });
    assert_eq!(resolution, (1, (3601, 3601)));
}

#[test]
fn test_tile_open() {
    let mut path = one_arcsecond_dir();
    path.push("N44W072.hgt");
    Tile::load(path).unwrap();
}

#[test]
fn test_out_of_bounds_get_returns_none() {
    let mut path = one_arcsecond_dir();
    path.push("N44W072.hgt");
    let tile = Tile::load(path).unwrap();
    // Assert coordinate a smidge north of tile returns None.
    assert_eq!(tile.get_geo(Coord { x: -71.5, y: 45.1 }), None);
    // Assert coordinate a smidge east of tile returns None.
    assert_eq!(tile.get_geo(Coord { x: -70.9, y: 44.5 }), None);
    // Assert coordinate a smidge south of tile returns None.
    assert_eq!(tile.get_geo(Coord { x: -71.5, y: 43.9 }), None);
    // Assert coordinate a smidge west of tile returns None.
    assert_eq!(tile.get_geo(Coord { x: -72.1, y: 44.5 }), None);
}

#[test]
fn test_tile_index() {
    let mut path = one_arcsecond_dir();
    path.push("N44W072.hgt");
    let raw_file_samples = {
        let mut file_data = Vec::new();
        let mut file = BufReader::new(File::open(&path).unwrap());
        while let Ok(sample) = util::read_sample(&mut file) {
            file_data.push(sample);
        }
        file_data
    };
    let parsed_tile = Tile::load(&path).unwrap();
    let mapped_tile = Tile::memmap(&path).unwrap();
    let mut idx = 0;
    for row in 0..3601 {
        for col in 0..3601 {
            assert_eq!(
                raw_file_samples[idx],
                parsed_tile.get_xy_unchecked((col, row))
            );
            assert_eq!(
                raw_file_samples[idx],
                mapped_tile.get_xy_unchecked((col, row))
            );
            idx += 1;
        }
    }
}

#[test]
fn test_tile_geo_index() {
    let mut path = one_arcsecond_dir();
    path.push("N44W072.hgt");
    let tile = Tile::load(&path).unwrap();
    let mt_washington = Coord {
        y: 44.2705,
        x: -71.30325,
    };
    assert_eq!(tile.get_geo_unchecked(mt_washington), tile.max_elevation());
}

#[test]
fn test_tile_index_conversions() {
    let mut path = one_arcsecond_dir();
    path.push("N44W072.hgt");
    let tile = Tile::load(&path).unwrap();

    assert_eq!((0, 0), tile.linear_to_xy(0));
    assert_eq!((0, 0), tile.geo_to_xy(Coord { x: -72.0, y: 45.0 }));
    assert_eq!((3600, 0), tile.geo_to_xy(Coord { x: -71.0, y: 45.0 }));
    assert_eq!((3600, 0), tile.linear_to_xy(3600));
    assert_eq!((0, 1), tile.linear_to_xy(3601));
    assert_eq!((0, 3600), tile.geo_to_xy(Coord { x: -72.0, y: 44.0 }));
    assert_eq!((3600, 3600), tile.geo_to_xy(Coord { x: -71.0, y: 44.0 }));

    assert_eq!(Coord { x: -71.0, y: 45.0 }, tile.xy_to_geo((3600, 0)));
    assert_eq!(Coord { x: -72.0, y: 45.0 }, tile.xy_to_geo((0, 0)));
    assert_eq!(Coord { x: -72.0, y: 44.0 }, tile.xy_to_geo((0, 3600)));
    assert_eq!(
        Coord { x: -72.0, y: 45.0 },
        tile.xy_to_geo(tile.linear_to_xy(0))
    );

    for row in 0..3601 {
        for col in 0..3601 {
            let linear = tile.xy_to_linear((col, row));
            let roundtrip_xy = tile.linear_to_xy(linear);
            assert_eq!((col, row), roundtrip_xy);
            let geo = tile.xy_to_geo((col, row));
            let roundtrip_xy = tile.geo_to_xy(geo);
            #[allow(clippy::cast_possible_wrap)]
            let xy = (col as isize, row as isize);
            assert_eq!(xy, roundtrip_xy);
        }
    }
}
