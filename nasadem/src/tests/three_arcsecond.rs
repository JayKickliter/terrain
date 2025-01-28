use crate::{
    geo::{geometry::LineString, Coord, Polygon},
    util, Tile,
};
use std::{fs::File, io::BufReader, path::PathBuf};

fn three_arcsecond_dir() -> PathBuf {
    [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "data",
        "nasadem",
        "3arcsecond",
    ]
    .iter()
    .collect()
}

#[test]
fn test_parse_hgt_name() {
    let mut path = three_arcsecond_dir();
    path.push("N44W072.hgt");
    let sw_corner = util::parse_sw_corner(&path).unwrap();
    let resolution = util::extract_resolution(&path).unwrap();
    assert_eq!(sw_corner, Coord { x: -72, y: 44 });
    assert_eq!(resolution, (3, (1201, 1201)));
}

#[test]
fn test_tile_open() {
    let mut path = three_arcsecond_dir();
    path.push("N44W072.hgt");
    Tile::load(path).unwrap();
}

#[test]
fn test_out_of_bounds_get_returns_none() {
    let mut path = three_arcsecond_dir();
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
    let mut path = three_arcsecond_dir();
    path.push("N44W072.hgt");
    let tile = Tile::load(&path).unwrap();
    let raw_file_samples = {
        let mut file_data = Vec::new();
        let mut raw = BufReader::new(File::open(path).unwrap());
        while let Ok(sample) = util::read_sample(&mut raw) {
            file_data.push(sample);
        }
        file_data
    };
    let mut idx = 0;
    for row in 0..1201 {
        for col in 0..1201 {
            assert_eq!(raw_file_samples[idx], tile.get_xy_unchecked((col, row)));
            idx += 1;
        }
    }
}

// #[test]
// fn test_tile_geo_index() {
//     let mut path = three_arcsecond_dir();
//     path.push("N44W072.hgt");
//     let tile = Tile::load(&path).unwrap();
//     let mt_washington = Coord {
//         y: 44.2705,
//         x: -71.30325,
//     };
//     // TODO: is there an error in indexing or is the 3 arc-second
//     //       dataset smeared?
//     assert_eq!(tile.get(mt_washington), Some(tile.max_elevation()));
// }

#[test]
fn test_tile_index_conversions() {
    let mut path = three_arcsecond_dir();
    path.push("N44W072.hgt");
    let tile = Tile::load(&path).unwrap();

    assert_eq!((0, 0), tile.linear_to_xy(0));
    assert_eq!((0, 0), tile.geo_to_xy(Coord { x: -72.0, y: 45.0 }));
    assert_eq!((1200, 0), tile.geo_to_xy(Coord { x: -71.0, y: 45.0 }));
    assert_eq!((1200, 0), tile.linear_to_xy(1200));
    assert_eq!((0, 1), tile.linear_to_xy(1201));
    assert_eq!((0, 1200), tile.geo_to_xy(Coord { x: -72.0, y: 44.0 }));
    assert_eq!((1200, 1200), tile.geo_to_xy(Coord { x: -71.0, y: 44.0 }));

    assert_eq!(Coord { x: -71.0, y: 45.0 }, tile.xy_to_geo((1200, 0)));
    assert_eq!(Coord { x: -72.0, y: 45.0 }, tile.xy_to_geo((0, 0)));
    assert_eq!(Coord { x: -72.0, y: 44.0 }, tile.xy_to_geo((0, 1200)));
    assert_eq!(
        Coord { x: -72.0, y: 45.0 },
        tile.xy_to_geo(tile.linear_to_xy(0))
    );

    for row in 0..1201 {
        for col in 0..1201 {
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

#[test]
fn test_xy_to_polygon() {
    let mut path = three_arcsecond_dir();
    path.push("N44W072.hgt");
    let parsed_tile = Tile::load(&path).unwrap();
    assert_eq!(
        parsed_tile.xy_to_polygon((0, 0)),
        Polygon::new(
            LineString::from(vec![
                (-72.000_416_666_666_67, 43.999_583_333_333_334),
                (-71.999_583_333_333_33, 43.999_583_333_333_334),
                (-71.999_583_333_333_33, 44.000_416_666_666_666),
                (-72.000_416_666_666_67, 44.000_416_666_666_666),
                (-72.000_416_666_666_67, 43.999_583_333_333_334),
            ]),
            vec![],
        )
    );
}
