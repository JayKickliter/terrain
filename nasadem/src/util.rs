use crate::{
    geo::{
        geometry::{Coord, Polygon},
        polygon,
    },
    Elev, NasademError, C, HALF_ARCSEC,
};
use std::path::Path;

pub(crate) fn extract_resolution<P: AsRef<Path>>(
    path: P,
) -> Result<(u8, (usize, usize)), NasademError> {
    const RES_1_ARCSECONDS_FILE_LEN: u64 = 3601 * 3601 * size_of::<u16>() as u64;
    const RES_3_ARCSECONDS_FILE_LEN: u64 = 1201 * 1201 * size_of::<u16>() as u64;
    match path.as_ref().metadata().map(|m| m.len())? {
        RES_1_ARCSECONDS_FILE_LEN => Ok((1, (3601, 3601))),
        RES_3_ARCSECONDS_FILE_LEN => Ok((3, (1201, 1201))),
        invalid_len => Err(NasademError::HgtLen(
            invalid_len,
            path.as_ref().to_path_buf(),
        )),
    }
}

pub(crate) fn parse_sw_corner<P: AsRef<Path>>(path: P) -> Result<Coord<Elev>, NasademError> {
    let mk_err = || NasademError::HgtName(path.as_ref().to_owned());
    let name = path
        .as_ref()
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(mk_err)?;
    if name.len() != 7 {
        return Err(mk_err());
    }
    let lat_sign = match &name[0..1] {
        "N" | "n" => 1,
        "S" | "s" => -1,
        _ => return Err(mk_err()),
    };
    let lat = lat_sign * name[1..3].parse::<Elev>().map_err(|_| mk_err())?;
    let lon_sign = match &name[3..4] {
        "E" | "e" => 1,
        "W" | "w" => -1,
        _ => return Err(mk_err()),
    };
    let lon = lon_sign * name[4..7].parse::<Elev>().map_err(|_| mk_err())?;
    Ok(Coord { x: lon, y: lat })
}

// Parses a big-endian Elev from a slice of two bytes.
//
// # Panics
//
// Panics if the provided slice is less than two bytes in lenght.
pub(crate) fn parse_sample(src: &[u8]) -> Elev {
    let mut sample_bytes = [0u8; 2];
    sample_bytes.copy_from_slice(src);
    Elev::from_be_bytes(sample_bytes)
}

// Reads a big-endian Elev from a slice of two bytes.
//
// # Panics
//
// Panics on IO error.
pub(crate) fn read_sample(src: &mut impl std::io::Read) -> std::io::Result<Elev> {
    let mut sample_bytes = [0u8; 2];
    src.read_exact(&mut sample_bytes)?;
    Ok(Elev::from_be_bytes(sample_bytes))
}

/// Generate a `res`-arcsecond square around `center`.
pub(crate) fn polygon(center: &Coord<C>, res: C) -> Polygon<C> {
    let delta = res * HALF_ARCSEC;
    let n = center.y + delta;
    let e = center.x + delta;
    let s = center.y - delta;
    let w = center.x - delta;
    polygon![
        (x: w, y: s),
        (x: e, y: s),
        (x: e, y: n),
        (x: w, y: n),
        (x: w, y: s),
    ]
}
