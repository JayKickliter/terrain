//! sRGB color type and HSL conversions shared by the shading paths.

use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};

/// An 8-bit sRGB color, serialized as `#rrggbb`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Rgb8 {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
}

impl Rgb8 {
    /// Constructs a color from its channels.
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Converts from HSL (`h` degrees, `s`/`l` percent).
    #[must_use]
    pub fn from_hsl(h: f32, s: f32, l: f32) -> Self {
        let [r, g, b] = hsl_to_rgb(h, s, l);
        Self { r, g, b }
    }

    /// HSL hue in degrees and saturation in percent.
    ///
    /// Lightness is dropped: shading supplies it per pixel.
    #[must_use]
    pub fn hue_sat(self) -> (f32, f32) {
        let r = f32::from(self.r) / 255.0;
        let g = f32::from(self.g) / 255.0;
        let b = f32::from(self.b) / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;
        if delta == 0.0 {
            return (0.0, 0.0);
        }
        let sum = max + min;
        let sat = if sum > 1.0 {
            delta / (2.0 - sum)
        } else {
            delta / sum
        };
        let hue = if max == r {
            (g - b) / delta + if g < b { 6.0 } else { 0.0 }
        } else if max == g {
            (b - r) / delta + 2.0
        } else {
            (r - g) / delta + 4.0
        };
        (hue * 60.0, sat * 100.0)
    }

    /// Parses `#rrggbb`, with the leading `#` optional.
    ///
    /// Returns `None` on any other shape.
    #[must_use]
    pub fn parse_hex(s: &str) -> Option<Self> {
        let digits = s.trim().strip_prefix('#').unwrap_or(s.trim());
        if digits.len() != 6 || !digits.is_ascii() {
            return None;
        }
        let r = u8::from_str_radix(&digits[0..2], 16).ok()?;
        let g = u8::from_str_radix(&digits[2..4], 16).ok()?;
        let b = u8::from_str_radix(&digits[4..6], 16).ok()?;
        Some(Self { r, g, b })
    }
}

impl std::fmt::Display for Rgb8 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

impl Serialize for Rgb8 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Rgb8 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse_hex(&text)
            .ok_or_else(|| D::Error::custom(format!("expected #rrggbb color, got {text}")))
    }
}

/// Converts HSL (`h` degrees, `s`/`l` percent) to 8-bit RGB.
#[must_use]
pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
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
    use super::Rgb8;

    #[test]
    fn parses_hex_with_and_without_hash() {
        assert_eq!(
            Rgb8::parse_hex("#43ab6d"),
            Some(Rgb8::new(0x43, 0xab, 0x6d))
        );
        assert_eq!(Rgb8::parse_hex("43AB6D"), Some(Rgb8::new(0x43, 0xab, 0x6d)));
        assert_eq!(
            Rgb8::parse_hex("  #43ab6d "),
            Some(Rgb8::new(0x43, 0xab, 0x6d))
        );
    }

    #[test]
    fn rejects_malformed_hex() {
        assert_eq!(Rgb8::parse_hex(""), None);
        assert_eq!(Rgb8::parse_hex("#abc"), None);
        assert_eq!(Rgb8::parse_hex("#gggggg"), None);
        assert_eq!(Rgb8::parse_hex("#43ab6d00"), None);
    }

    #[test]
    fn round_trips_hex() {
        let color = Rgb8::new(1, 128, 255);
        assert_eq!(color.to_string(), "#0180ff");
        assert_eq!(Rgb8::parse_hex(&color.to_string()), Some(color));
    }

    /// Hue/sat must survive an HSL round trip at mid lightness.
    #[test]
    fn round_trips_hue_sat() {
        for &(hue, sat) in &[(36.0, 92.0), (145.0, 63.0), (283.0, 37.0), (204.0, 64.0)] {
            let (h, s) = Rgb8::from_hsl(hue, sat, 50.0).hue_sat();
            assert!((h - hue).abs() < 1.0, "hue {h} drifted from {hue}");
            assert!((s - sat).abs() < 1.0, "sat {s} drifted from {sat}");
        }
    }

    #[test]
    fn gray_has_no_hue() {
        assert_eq!(Rgb8::new(128, 128, 128).hue_sat(), (0.0, 0.0));
    }

    #[test]
    fn serde_uses_hex_string() {
        let color = Rgb8::new(0x43, 0xab, 0x6d);
        let text = toml::to_string(&toml::toml! { c = "#43ab6d" }).unwrap();
        let back: std::collections::BTreeMap<String, Rgb8> = toml::from_str(&text).unwrap();
        assert_eq!(back["c"], color);
    }
}
