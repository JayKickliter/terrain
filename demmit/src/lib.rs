use image::{ImageBuffer, Rgb};
use nalgebra::{DMatrix, Scalar};
use nasadem::Tile;
// use palette::{convert::FromColorUnclamped, Hsl, Srgb};
use colors_transform::{Color, Hsl};
use std::f32::consts::FRAC_PI_2;

mod worldcover;

pub use worldcover::{tile_to_worldcover_matrix, WorldCover};

pub fn tile_to_matrix<T>(tile: &Tile) -> DMatrix<T>
where
    T: From<i16> + Scalar,
{
    let (w, h) = tile.dimensions();
    DMatrix::from_row_iterator(h, w, tile.iter().map(|sample| T::from(sample.elevation())))
}

pub fn apply_shading(sun_az_rad: f32, sun_elev_rad: f32, data: &DMatrix<f32>) -> DMatrix<f32> {
    // Translate from azimuth (clockwise starting at due north) to
    // conventional math angle (counter clockwise from y=0 and x>0).
    let sun_angle_rad = -(std::f32::consts::FRAC_PI_2 - sun_az_rad);
    let (rows, cols) = data.shape();
    let mut out = DMatrix::zeros(rows, cols);
    let (rows, cols) = (
        u16::try_from(rows).expect("unexpected size"),
        u16::try_from(cols).expect("unexpected size"),
    );

    let get = |x: i32, y: i32| {
        let x = x.clamp(0, i32::from(cols - 1));
        let y = y.clamp(0, i32::from(rows - 1));
        data.index((
            usize::try_from(y).expect("unexpected size"),
            usize::try_from(x).expect("unexpected size"),
        ))
    };

    for x in 0..i32::from(cols) {
        for y in 0..i32::from(rows) {
            let (aspect, slope) = {
                let dzdx = get(x + 1, y) - get(x - 1, y);
                let dzdy = get(x, y + 1) - get(x, y - 1);
                let slope = ((dzdx.powi(2) + dzdy.powi(2)).sqrt() / 30.0).atan();
                assert!(slope.is_finite());
                assert!(slope.is_sign_positive());
                let aspect = f32::atan2(-dzdy, -dzdx);
                assert!(slope.is_finite());
                (aspect, slope)
            };
            let reflection =
                (aspect - sun_angle_rad).cos() * (slope).sin() * (FRAC_PI_2 - sun_elev_rad).sin()
                    + slope.cos() * (FRAC_PI_2 - sun_elev_rad).cos();
            assert!(reflection.is_finite());
            assert!(reflection <= 1.0);
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
        let clamped = slope.max(0.0);
        // Reduce dynamic range a little by attenuating all values and
        // adding a little bit ambient light.
        let lum = clamped * 0.9 + 0.1;
        assert!(lum >= 0.0);
        assert!(lum <= 1.0);
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
