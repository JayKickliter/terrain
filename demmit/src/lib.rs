use colors_transform::{Color, Hsl};
use image::{ImageBuffer, Luma, Rgb};
use nalgebra::{DMatrix, Scalar};
use nasadem::Tile;
use std::f32::consts::FRAC_PI_2;

mod worldcover;

pub use worldcover::{tile_to_worldcover_matrix, WorldCover};

/// Approximate ground distance of one arcsecond at the equator, in meters.
pub const METERS_PER_ARCSEC: f32 = 30.87;

/// Fraction of illumination from directional light.
const DIRECT_LIGHT: f32 = 0.9;

/// Fraction of illumination from ambient light.
const AMBIENT_LIGHT: f32 = 0.1;

pub fn tile_to_matrix<T>(tile: &Tile) -> DMatrix<T>
where
    T: From<i16> + Scalar,
{
    let (w, h) = tile.dimensions();
    DMatrix::from_row_iterator(h, w, tile.iter().map(|sample| T::from(sample.elevation())))
}

/// Computes hillshade reflectance per cell using a Sobel 3x3 kernel.
///
/// `cell_size`: ground distance between adjacent samples in meters.
/// Returns values in roughly [-1.0, 1.0].
///
/// # Panics
///
/// Panics if either dimension exceeds `u16::MAX`.
pub fn apply_shading(
    sun_az_rad: f32,
    sun_elev_rad: f32,
    cell_size: f32,
    data: &DMatrix<f32>,
) -> DMatrix<f32> {
    // Compass azimuth (CW from north) to math angle (CCW from east)
    let sun_angle_rad = FRAC_PI_2 - sun_az_rad;
    let zenith_rad = FRAC_PI_2 - sun_elev_rad;
    let cos_z = zenith_rad.cos();
    let sin_z = zenith_rad.sin();

    let (rows, cols) = data.shape();
    let mut out = DMatrix::zeros(rows, cols);
    let (rows, cols) = (
        u16::try_from(rows).expect("unexpected size"),
        u16::try_from(cols).expect("unexpected size"),
    );

    let get = |x: i32, y: i32| {
        let x = x.clamp(0, i32::from(cols - 1));
        let y = y.clamp(0, i32::from(rows - 1));
        *data.index((
            usize::try_from(y).expect("unexpected size"),
            usize::try_from(x).expect("unexpected size"),
        ))
    };

    // Sobel kernel absolute weight sum (1+2+1 per side)
    let sobel_norm = 8.0 * cell_size;

    for x in 0..i32::from(cols) {
        for y in 0..i32::from(rows) {
            let nw = get(x - 1, y - 1);
            let n  = get(x,     y - 1);
            let ne = get(x + 1, y - 1);
            let w  = get(x - 1, y);
            let e  = get(x + 1, y);
            let sw = get(x - 1, y + 1);
            let s  = get(x,     y + 1);
            let se = get(x + 1, y + 1);

            let dzdx = ((ne + 2.0 * e + se) - (nw + 2.0 * w + sw)) / sobel_norm;
            let dzdy = ((sw + 2.0 * s + se) - (nw + 2.0 * n + ne)) / sobel_norm;

            let slope = (dzdx.powi(2) + dzdy.powi(2)).sqrt().atan();
            let aspect = f32::atan2(dzdy, -dzdx);

            let reflection =
                cos_z * slope.cos() + sin_z * slope.sin() * (sun_angle_rad - aspect).cos();

            #[allow(clippy::cast_sign_loss)]
            {
                *out.index_mut((y as usize, x as usize)) = reflection;
            }
        }
    }
    out
}

pub fn matrix_to_wc_image(
    worldcover_mat: &DMatrix<WorldCover>,
    slope_mat: &DMatrix<f32>,
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let (rows, cols) = slope_mat.shape();
    let (rows, cols) = (
        u16::try_from(rows).expect("unexpected size"),
        u16::try_from(cols).expect("unexpected size"),
    );

    let f = |col, row| {
        let slope = *slope_mat.index((row as usize, col as usize));
        let worldcover = *worldcover_mat.index((row as usize, col as usize));
        let lum = (slope.max(0.0) * DIRECT_LIGHT + AMBIENT_LIGHT).clamp(0.0, 1.0);
        let (hue, sat) = match worldcover {
            WorldCover::Bare => (36, 92),
            WorldCover::Built => (209, 11),
            WorldCover::Crop => (28, 80),
            WorldCover::Frozen => (180, 14),
            WorldCover::Grass => (42, 46),
            WorldCover::Mangrove => (203, 51),
            WorldCover::Moss => (283, 37),
            WorldCover::Shrub => (54, 45),
            WorldCover::Tree => (145, 63),
            WorldCover::Water => (204, 64),
            WorldCover::Wet => (204, 66),
        };
        #[allow(clippy::cast_precision_loss)]
        let hsl = Hsl::from(hue as f32, sat as f32, lum * 100.0);
        let rgb = hsl.to_rgb();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (r, g, b) = (
            rgb.get_red() as u8,
            rgb.get_green() as u8,
            rgb.get_blue() as u8,
        );
        Rgb([r, g, b])
    };
    ImageBuffer::from_fn(u32::from(cols), u32::from(rows), f)
}

/// Renders a shading matrix as a grayscale image.
///
/// # Panics
///
/// Panics if either dimension exceeds `u16::MAX`.
pub fn matrix_to_grayscale(shading_mat: &DMatrix<f32>) -> ImageBuffer<Luma<u8>, Vec<u8>> {
    let (rows, cols) = shading_mat.shape();
    let (rows, cols) = (
        u16::try_from(rows).expect("unexpected size"),
        u16::try_from(cols).expect("unexpected size"),
    );
    ImageBuffer::from_fn(u32::from(cols), u32::from(rows), |col, row| {
        let val = *shading_mat.index((row as usize, col as usize));
        let lum = (val.max(0.0) * DIRECT_LIGHT + AMBIENT_LIGHT).clamp(0.0, 1.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Luma([(lum * 255.0) as u8])
    })
}

#[allow(clippy::cast_precision_loss)]
pub fn pyramid(rows: usize, cols: usize) -> DMatrix<f32> {
    let mut out = DMatrix::zeros(rows, cols);
    for x in 0..cols {
        let x = if x < cols / 2 { x } else { cols - 1 - x };
        for y in 0..rows {
            let y = if y < rows / 2 { y } else { rows - 1 - y };
            *out.index_mut((y, x)) = (x + y) as f32 / 4.0;
        }
    }
    out
}

#[allow(clippy::cast_precision_loss)]
pub fn dome(rows: usize, cols: usize) -> DMatrix<f32> {
    let mut out = DMatrix::zeros(rows, cols);
    for x in 0..cols {
        let xx = (x as f32 - cols as f32 / 2.0) / (cols as f32 / 2.0);
        for y in 0..rows {
            let yy = (y as f32 - rows as f32 / 2.0) / (rows as f32 / 2.0);
            let elev = (1.0 - (xx.powi(2) + yy.powi(2))).sqrt() * 1600.0;
            *out.index_mut((y, x)) = elev;
        }
    }
    out
}
