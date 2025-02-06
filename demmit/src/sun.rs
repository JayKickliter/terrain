//! Sun-position dial: a circle where the edge is 0deg elevation, the
//! center is 90deg (overhead), and angle around the circle is azimuth
//! (north up, clockwise).

use demmit::Sun;
use eframe::egui::{pos2, Pos2};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// Geometry of the sun dial in screen space.
#[derive(Clone, Copy)]
pub struct SunDial {
    /// Circle center in screen pixels.
    pub center: Pos2,
    /// Circle radius in pixels.
    pub radius: f32,
}

impl SunDial {
    /// Screen position of the sun handle for the given sun.
    pub fn handle_pos(&self, sun: Sun) -> Pos2 {
        let d = self.radius * (1.0 - sun.elev_rad / FRAC_PI_2);
        let ox = d * sun.az_rad.sin();
        let oy = -d * sun.az_rad.cos();
        pos2(self.center.x + ox, self.center.y + oy)
    }

    /// Sun position implied by a handle at screen point `p`.
    ///
    /// Points outside the circle clamp to the horizon (0deg elevation).
    pub fn sun_from_pos(&self, p: Pos2) -> Sun {
        let (ox, oy) = (p.x - self.center.x, p.y - self.center.y);
        let d = (ox * ox + oy * oy).sqrt().min(self.radius);
        let mut az = ox.atan2(-oy);
        if az < 0.0 {
            az += TAU;
        }
        let elev = FRAC_PI_2 * (1.0 - d / self.radius);
        Sun {
            az_rad: az,
            elev_rad: elev,
        }
    }

    /// Whether `p` lies within grab distance of the dial.
    pub fn contains(&self, p: Pos2, slop: f32) -> bool {
        let (ox, oy) = (p.x - self.center.x, p.y - self.center.y);
        (ox * ox + oy * oy).sqrt() <= self.radius + slop
    }
}

/// Azimuth tick directions in radians, N/E/S/W.
pub const CARDINALS: [(f32, &str); 4] = [
    (0.0, "N"),
    (FRAC_PI_2, "E"),
    (PI, "S"),
    (PI + FRAC_PI_2, "W"),
];

#[cfg(test)]
mod tests {
    use super::SunDial;
    use demmit::Sun;
    use eframe::egui::pos2;
    use std::f32::consts::FRAC_PI_2;

    fn dial() -> SunDial {
        SunDial {
            center: pos2(100.0, 100.0),
            radius: 50.0,
        }
    }

    #[test]
    fn center_is_overhead() {
        let sun = dial().sun_from_pos(pos2(100.0, 100.0));
        assert!((sun.elev_rad - FRAC_PI_2).abs() < 1e-4);
    }

    #[test]
    fn edge_is_horizon() {
        let sun = dial().sun_from_pos(pos2(150.0, 100.0));
        assert!(sun.elev_rad.abs() < 1e-4);
    }

    #[test]
    fn north_is_up() {
        let sun = dial().sun_from_pos(pos2(100.0, 60.0));
        assert!(sun.az_rad.abs() < 1e-4 || (sun.az_rad - std::f32::consts::TAU).abs() < 1e-4);
    }

    #[test]
    fn east_is_right() {
        let sun = dial().sun_from_pos(pos2(140.0, 100.0));
        assert!((sun.az_rad - FRAC_PI_2).abs() < 1e-4);
    }

    #[test]
    fn round_trips() {
        let d = dial();
        for &(az_deg, elev_deg) in &[(315.0, 45.0), (30.0, 10.0), (200.0, 80.0)] {
            let sun = Sun {
                az_rad: (az_deg as f32).to_radians(),
                elev_rad: (elev_deg as f32).to_radians(),
            };
            let back = d.sun_from_pos(d.handle_pos(sun));
            assert!((back.az_rad - sun.az_rad).abs() < 1e-3, "az {az_deg}");
            assert!(
                (back.elev_rad - sun.elev_rad).abs() < 1e-3,
                "elev {elev_deg}"
            );
        }
    }
}
