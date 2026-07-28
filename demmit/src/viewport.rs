//! Equirectangular map viewport: center in degrees plus pixels-per-degree zoom.

use eframe::egui::{pos2, Pos2, Rect};

/// Map view state.
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    /// Center longitude in degrees.
    pub center_lon: f64,
    /// Center latitude in degrees.
    pub center_lat: f64,
    /// Screen pixels per degree (zoom).
    pub ppd: f64,
}

impl Viewport {
    /// Projects a geographic point to a screen position within `area`.
    pub fn world_to_screen(&self, area: Rect, lon: f64, lat: f64) -> Pos2 {
        let c = area.center();
        let x = f64::from(c.x) + (lon - self.center_lon) * self.ppd;
        let y = f64::from(c.y) - (lat - self.center_lat) * self.ppd;
        pos2(x as f32, y as f32)
    }

    /// Screen rect covering the 1-degree tile with the given SW corner.
    pub fn tile_rect(&self, area: Rect, lon_sw: i32, lat_sw: i32) -> Rect {
        let nw = self.world_to_screen(area, f64::from(lon_sw), f64::from(lat_sw + 1));
        let se = self.world_to_screen(area, f64::from(lon_sw + 1), f64::from(lat_sw));
        Rect::from_two_pos(nw, se)
    }

    /// Integer SW corners of all tiles overlapping `area`.
    ///
    /// `margin` extends the range by that many tiles on each side, for
    /// prefetching just outside the view.
    pub fn visible_tiles(&self, area: Rect, margin: i32) -> Vec<(i32, i32)> {
        let half_w = f64::from(area.width()) / 2.0 / self.ppd;
        let half_h = f64::from(area.height()) / 2.0 / self.ppd;
        let lon_lo = (self.center_lon - half_w).floor() as i32 - margin;
        let lon_hi = (self.center_lon + half_w).floor() as i32 + margin;
        let lat_lo = (self.center_lat - half_h).floor() as i32 - margin;
        let lat_hi = (self.center_lat + half_h).floor() as i32 + margin;
        let mut out = Vec::new();
        for lat in lat_lo..=lat_hi {
            for lon in lon_lo..=lon_hi {
                out.push((lon, lat));
            }
        }
        out
    }

    /// Pans the view by a screen-space delta in pixels.
    pub fn pan_pixels(&mut self, dx: f32, dy: f32) {
        self.center_lon -= f64::from(dx) / self.ppd;
        self.center_lat += f64::from(dy) / self.ppd;
    }

    /// Zooms by `factor` about a screen anchor, keeping that point fixed.
    pub fn zoom_about(&mut self, area: Rect, anchor: Pos2, factor: f64) {
        let before = self.screen_to_world(area, anchor);
        self.ppd = (self.ppd * factor).clamp(MIN_PPD, MAX_PPD);
        let after = self.screen_to_world(area, anchor);
        self.center_lon += before.0 - after.0;
        self.center_lat += before.1 - after.1;
    }

    /// Fits the screen box spanning `a`..`b` to `area`, best effort.
    ///
    /// Aspect ratio is not preserved between the box and `area`; zoom is
    /// chosen so the whole box stays visible. No-ops for a degenerate box.
    pub fn zoom_to_box(&mut self, area: Rect, a: Pos2, b: Pos2) {
        let (lon0, lat0) = self.screen_to_world(area, a);
        let (lon1, lat1) = self.screen_to_world(area, b);
        let (w, h) = ((lon1 - lon0).abs(), (lat1 - lat0).abs());
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        self.center_lon = f64::midpoint(lon0, lon1);
        self.center_lat = f64::midpoint(lat0, lat1);
        let fit = (f64::from(area.width()) / w).min(f64::from(area.height()) / h);
        self.ppd = fit.clamp(MIN_PPD, MAX_PPD);
    }

    /// Inverse projection: screen position to `(lon, lat)`.
    pub fn screen_to_world(&self, area: Rect, p: Pos2) -> (f64, f64) {
        let c = area.center();
        let lon = self.center_lon + f64::from(p.x - c.x) / self.ppd;
        let lat = self.center_lat - f64::from(p.y - c.y) / self.ppd;
        (lon, lat)
    }
}

/// Minimum zoom: whole tile spans this many pixels.
const MIN_PPD: f64 = 16.0;

/// Maximum zoom in pixels per degree.
const MAX_PPD: f64 = 200_000.0;

#[cfg(test)]
mod tests {
    use super::Viewport;
    use eframe::egui::{pos2, vec2, Pos2, Rect};

    fn area() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 800.0))
    }

    fn vp() -> Viewport {
        Viewport {
            center_lon: -71.5,
            center_lat: 44.5,
            ppd: 500.0,
        }
    }

    fn screen_to_world(vp: &Viewport, a: Rect, p: Pos2) -> (f64, f64) {
        let c = a.center();
        let lon = vp.center_lon + f64::from(p.x - c.x) / vp.ppd;
        let lat = vp.center_lat - f64::from(p.y - c.y) / vp.ppd;
        (lon, lat)
    }

    #[test]
    fn center_maps_to_area_center() {
        let p = vp().world_to_screen(area(), -71.5, 44.5);
        assert!((p.x - 500.0).abs() < 1e-3);
        assert!((p.y - 400.0).abs() < 1e-3);
    }

    #[test]
    fn north_is_up() {
        let (a, vp) = (area(), vp());
        let south = vp.world_to_screen(a, -71.5, 44.0);
        let north = vp.world_to_screen(a, -71.5, 45.0);
        assert!(north.y < south.y, "north should be higher on screen");
    }

    #[test]
    fn tile_rect_is_one_degree_square() {
        let r = vp().tile_rect(area(), -72, 44);
        assert!((r.width() - 500.0).abs() < 1e-3);
        assert!((r.height() - 500.0).abs() < 1e-3);
    }

    #[test]
    fn visible_includes_center_tile() {
        assert!(vp().visible_tiles(area(), 0).contains(&(-72, 44)));
    }

    #[test]
    fn margin_expands_tile_range() {
        let base = vp().visible_tiles(area(), 0).len();
        let expanded = vp().visible_tiles(area(), 1).len();
        assert!(expanded > base);
    }

    #[test]
    fn zoom_about_keeps_anchor_fixed() {
        let a = area();
        let mut vp = vp();
        let anchor = pos2(250.0, 300.0);
        let (lon, lat) = screen_to_world(&vp, a, anchor);
        vp.zoom_about(a, anchor, 2.0);
        let after = vp.world_to_screen(a, lon, lat);
        assert!((after.x - anchor.x).abs() < 1.0);
        assert!((after.y - anchor.y).abs() < 1.0);
    }

    #[test]
    fn zoom_to_box_centers_and_fits() {
        let a = area();
        let mut vp = vp();
        let (p0, p1) = (pos2(200.0, 150.0), pos2(700.0, 650.0));
        let mid = pos2(f32::midpoint(p0.x, p1.x), f32::midpoint(p0.y, p1.y));
        let (mid_lon, mid_lat) = screen_to_world(&vp, a, mid);
        vp.zoom_to_box(a, p0, p1);
        // Box center becomes the view center.
        let c = vp.world_to_screen(a, mid_lon, mid_lat);
        assert!((c.x - a.center().x).abs() < 1.0);
        assert!((c.y - a.center().y).abs() < 1.0);
        // Box is 500x500 px at ppd 500 = 1x1 degree; fit into 1000x800
        // is min(1000, 800) = 800 ppd (height-limited).
        assert!((vp.ppd - 800.0).abs() < 1e-6);
    }

    #[test]
    fn zoom_to_box_ignores_degenerate() {
        let a = area();
        let mut vp = vp();
        let before = vp.ppd;
        vp.zoom_to_box(a, pos2(300.0, 300.0), pos2(300.0, 400.0));
        assert_eq!(vp.ppd, before);
    }
}
