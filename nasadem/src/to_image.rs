use crate::{Elev, Tile};
use image::{ImageBuffer, Luma};
use num_traits::AsPrimitive;

impl Tile {
    /// Returns an [`ImageBuffer`] of this tile.
    ///
    /// The image is scaled so that the lowest elevation is `0` and
    /// the highest is [`u16::MAX`].
    ///
    /// The original, pre-scaled, elevation can be computed with:
    /// `(pixel_value / 16::MAX) * (max_elev - min_elev) + min_elev`
    ///
    #[allow(clippy::cast_possible_truncation)]
    pub fn to_image<Pix>(&self) -> ImageBuffer<Luma<Pix>, Vec<Pix>>
    where
        Pix: image::Primitive + 'static,
        f32: AsPrimitive<Pix> + From<Pix>,
    {
        let (x_dim, y_dim) = self.dimensions();
        let mut img = ImageBuffer::new(x_dim as u32, y_dim as u32);
        let min_elev: f32 = self.min_elevation().into();
        let max_elev: f32 = self.max_elevation().into();
        let scale = |elev: Elev| {
            let elev: f32 = elev.into();
            (elev - min_elev) / (max_elev - min_elev) * f32::from(Pix::max_value())
        };
        for sample in self.iter() {
            let (x, y) = sample.xy();
            let elev = sample.elevation();
            let scaled_elev = scale(elev);
            #[allow(clippy::cast_sign_loss)]
            img.put_pixel(x as u32, y as u32, Luma([scaled_elev.as_()]));
        }
        img
    }
}
