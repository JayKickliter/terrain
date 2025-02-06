//! SIMD-friendly hillshade: precompute gradients once, reshade cheaply on sun/z change.
//!
//! The standard hillshade reflectance equals the dot of the surface
//! normal with the light vector. Factoring it that way lets sun and
//! z-factor changes reshade with only multiplies and one sqrt per
//! pixel (no per-pixel trig), which vectorizes well.

use crate::{WorldCover, AMBIENT_LIGHT, DIRECT_LIGHT, METERS_PER_ARCSEC};
use multiversion::multiversion;
use nasadem::Tile;
use std::f32::consts::FRAC_PI_2;

/// Sun position for shading.
#[derive(Clone, Copy, Debug)]
pub struct Sun {
    /// Compass azimuth in radians, clockwise from north.
    pub az_rad: f32,
    /// Elevation above the horizon in radians (0 = horizon, pi/2 = overhead).
    pub elev_rad: f32,
}

impl Sun {
    /// Returns the light unit vector `[lx, ly, lz]` for this sun position.
    ///
    /// Paired with a surface normal `(-dzdx, dzdy, 1)`, the reflectance
    /// is `(-lx*dzdx + ly*dzdy + lz) / |normal|`.
    fn light(self) -> [f32; 3] {
        let sun_angle = FRAC_PI_2 - self.az_rad;
        let zenith = FRAC_PI_2 - self.elev_rad;
        let cos_z = zenith.cos();
        let sin_z = zenith.sin();
        [sin_z * sun_angle.cos(), sin_z * sun_angle.sin(), cos_z]
    }
}

/// Per-cell elevation gradients for one tile, cached across reshades.
///
/// Gradients already fold in ground cell size, so only the sun and
/// z-factor vary per reshade.
pub struct Gradients {
    dzdx: Vec<f32>,
    dzdy: Vec<f32>,
    width: usize,
    height: usize,
}

impl Gradients {
    /// Builds gradients from a loaded tile using a Sobel 3x3 kernel.
    pub fn from_tile(tile: &Tile) -> Self {
        let (width, height) = tile.dimensions();
        let mut elev = vec![0.0_f32; width * height];
        for (dst, sample) in elev.iter_mut().zip(tile.iter()) {
            *dst = f32::from(sample.elevation());
        }
        let cell_size = f32::from(tile.resolution()) * METERS_PER_ARCSEC;
        Self::from_elevations(&elev, width, height, cell_size)
    }

    /// Builds gradients at a reduced display resolution by nearest-sampling `tile`.
    ///
    /// `out_px` is clamped to the tile's native resolution, so zooming
    /// past native never upsamples.
    pub fn from_tile_downsampled(tile: &Tile, out_px: usize) -> Self {
        let (nw, nh) = tile.dimensions();
        let px = out_px.clamp(2, nw.min(nh));
        let mut elev = vec![0.0_f32; px * px];
        let denom = (px - 1).max(1);
        for oy in 0..px {
            let ny = oy * (nh - 1) / denom;
            for ox in 0..px {
                let nx = ox * (nw - 1) / denom;
                elev[oy * px + ox] = f32::from(tile.get_unchecked((nx, ny)));
            }
        }
        let factor = nw as f32 / px as f32;
        let cell_size = f32::from(tile.resolution()) * METERS_PER_ARCSEC * factor;
        Self::from_elevations(&elev, px, px, cell_size)
    }

    /// Builds gradients from a row-major (NW-origin) elevation buffer.
    ///
    /// # Panics
    ///
    /// Panics if `elev.len() != width * height`.
    pub fn from_elevations(elev: &[f32], width: usize, height: usize, cell_size: f32) -> Self {
        assert_eq!(elev.len(), width * height, "elevation buffer size mismatch");
        let sobel_norm = 8.0 * cell_size;
        let mut dzdx = vec![0.0_f32; width * height];
        let mut dzdy = vec![0.0_f32; width * height];
        let last_x = width as i32 - 1;
        let last_y = height as i32 - 1;
        let get = |x: i32, y: i32| {
            let x = x.clamp(0, last_x) as usize;
            let y = y.clamp(0, last_y) as usize;
            elev[y * width + x]
        };
        for y in 0..height {
            for x in 0..width {
                let (xi, yi) = (x as i32, y as i32);
                let nw = get(xi - 1, yi - 1);
                let n = get(xi, yi - 1);
                let ne = get(xi + 1, yi - 1);
                let w = get(xi - 1, yi);
                let e = get(xi + 1, yi);
                let sw = get(xi - 1, yi + 1);
                let s = get(xi, yi + 1);
                let se = get(xi + 1, yi + 1);
                let idx = y * width + x;
                dzdx[idx] = ((ne + 2.0 * e + se) - (nw + 2.0 * w + sw)) / sobel_norm;
                dzdy[idx] = ((sw + 2.0 * s + se) - (nw + 2.0 * n + ne)) / sobel_norm;
            }
        }
        Self {
            dzdx,
            dzdy,
            width,
            height,
        }
    }

    /// Returns tile `(width, height)` in samples.
    pub fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// Number of cells (`width * height`).
    pub fn len(&self) -> usize {
        self.dzdx.len()
    }

    /// Whether the gradient buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.dzdx.is_empty()
    }

    /// Reshades to grayscale RGBA (`width * height * 4` bytes).
    ///
    /// # Panics
    ///
    /// Panics if `out.len() != self.len() * 4`.
    pub fn shade_grayscale(&self, sun: Sun, z_factor: f32, out: &mut [u8]) {
        assert_eq!(out.len(), self.len() * 4, "output buffer size mismatch");
        shade_grayscale_kernel(&self.dzdx, &self.dzdy, sun.light(), z_factor, out);
    }

    /// Reshades to RGBA tinted by per-cell land cover class.
    ///
    /// # Panics
    ///
    /// Panics if `cover.len() != self.len()` or `out.len() != self.len() * 4`.
    pub fn shade_worldcover(&self, cover: &[WorldCover], sun: Sun, z_factor: f32, out: &mut [u8]) {
        assert_eq!(cover.len(), self.len(), "cover buffer size mismatch");
        assert_eq!(out.len(), self.len() * 4, "output buffer size mismatch");
        let [lx, ly, lz] = sun.light();
        for (i, px) in out.chunks_exact_mut(4).enumerate() {
            let lum = reflectance(self.dzdx[i], self.dzdy[i], lx, ly, lz, z_factor);
            let (hue, sat) = cover[i].hue_sat();
            let [r, g, b] = hsl_to_rgb(hue, sat, lum * 100.0);
            px.copy_from_slice(&[r, g, b, 255]);
        }
    }
}

/// Grayscale reflectance in `[0, 1]` after direct/ambient mix.
#[inline]
fn reflectance(dzdx: f32, dzdy: f32, lx: f32, ly: f32, lz: f32, z: f32) -> f32 {
    let a = z * dzdx;
    let b = z * dzdy;
    let refl = (-lx * a + ly * b + lz) / (a * a + b * b + 1.0).sqrt();
    (refl.max(0.0) * DIRECT_LIGHT + AMBIENT_LIGHT).clamp(0.0, 1.0)
}

#[multiversion(targets = "simd")]
fn shade_grayscale_kernel(dzdx: &[f32], dzdy: &[f32], light: [f32; 3], z: f32, out: &mut [u8]) {
    let [lx, ly, lz] = light;
    for (i, px) in out.chunks_exact_mut(4).enumerate() {
        let lum = reflectance(dzdx[i], dzdy[i], lx, ly, lz, z);
        let v = (lum * 255.0) as u8;
        px.copy_from_slice(&[v, v, v, 255]);
    }
}

/// Converts HSL (`h` deg, `s`/`l` percent) to 8-bit RGB.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    let h = h / 360.0;
    let s = s / 100.0;
    let l = l / 100.0;
    if s == 0.0 {
        let v = (l * 255.0) as u8;
        return [v, v, v];
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let r = hue_to_channel(p, q, h + 1.0 / 3.0);
    let g = hue_to_channel(p, q, h);
    let b = hue_to_channel(p, q, h - 1.0 / 3.0);
    [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8]
}

#[inline]
fn hue_to_channel(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 1.0 / 2.0 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

#[cfg(test)]
mod tests {
    use super::{Gradients, Sun};
    use crate::{
        apply_shading, matrix_to_grayscale, tile_to_matrix, WorldCover, METERS_PER_ARCSEC,
    };
    use nasadem::Tile;

    #[test]
    fn worldcover_shade_fills_opaque_rgba() {
        let elev: Vec<f32> = (0..16 * 16).map(|i| (i % 7) as f32).collect();
        let grad = Gradients::from_elevations(&elev, 16, 16, 30.0);
        let cover = vec![WorldCover::Tree; grad.len()];
        let mut rgba = vec![0_u8; grad.len() * 4];
        let sun = Sun {
            az_rad: 315.0_f32.to_radians(),
            elev_rad: 45.0_f32.to_radians(),
        };
        grad.shade_worldcover(&cover, sun, 1.0, &mut rgba);
        assert!(rgba.chunks_exact(4).all(|px| px[3] == 255));
        assert!(rgba
            .chunks_exact(4)
            .any(|px| px[0] != px[1] || px[1] != px[2]));
    }

    /// Gradient reshade must match the trusted `apply_shading` path.
    #[test]
    fn grayscale_matches_apply_shading() {
        let path = format!(
            "{}/../data/nasadem/1arcsecond/N38W105.hgt",
            env!("CARGO_MANIFEST_DIR")
        );
        let tile = Tile::load(path).unwrap();
        let sun = Sun {
            az_rad: 315.0_f32.to_radians(),
            elev_rad: 45.0_f32.to_radians(),
        };
        let cell = f32::from(tile.resolution()) * METERS_PER_ARCSEC;

        let mat = tile_to_matrix(&tile);
        let old = matrix_to_grayscale(&apply_shading(sun.az_rad, sun.elev_rad, cell, &mat));

        let grad = Gradients::from_tile(&tile);
        let (w, h) = grad.dimensions();
        let mut buf = vec![0_u8; w * h * 4];
        grad.shade_grayscale(sun, 1.0, &mut buf);

        let mut max_diff = 0_i32;
        for (i, px) in buf.chunks_exact(4).enumerate() {
            let (x, y) = ((i % w) as u32, (i / w) as u32);
            let old_v = i32::from(old.get_pixel(x, y).0[0]);
            max_diff = max_diff.max((old_v - i32::from(px[0])).abs());
        }
        assert!(max_diff <= 1, "max pixel diff {max_diff} exceeds tolerance");
    }
}
