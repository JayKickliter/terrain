//! Persisted viewer state in `demmit.toml` under the OS config dir.

use camino::Utf8PathBuf;
use demmit::{CoverMask, Palette};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

/// First-run map center: Pikes Peak, Colorado.
const DEFAULT_LAT: f64 = 38.8405;
const DEFAULT_LON: f64 = -105.0442;

/// Coloring mode for shaded tiles.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Coloring {
    /// Grayscale hillshade.
    #[default]
    Grayscale,
    /// Hillshade tinted by land-cover class.
    Worldcover,
}

/// Saved window geometry.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct Window {
    /// Inner width in points.
    pub width: f32,
    /// Inner height in points.
    pub height: f32,
    /// Outer left position, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f32>,
    /// Outer top position, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f32>,
}

impl Default for Window {
    fn default() -> Self {
        Self {
            width: 1200.0,
            height: 800.0,
            x: None,
            y: None,
        }
    }
}

/// Persisted viewer configuration.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct Config {
    /// Last map center latitude.
    pub center_lat: f64,
    /// Last map center longitude.
    pub center_lon: f64,
    /// Pixels per degree (zoom).
    pub ppd: f64,
    /// Vertical exaggeration.
    pub z_factor: f32,
    /// Sun azimuth in degrees.
    pub sun_az_deg: f32,
    /// Sun elevation in degrees.
    pub sun_elev_deg: f32,
    /// Active coloring mode.
    pub coloring: Coloring,
    /// Path to the `WorldCover` h3 disktree, if configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worldcover_h3db: Option<Utf8PathBuf>,
    /// Tile directories, finest resolution first.
    pub dirs: Vec<Utf8PathBuf>,
    /// Per-class land-cover tint colors.
    pub cover_colors: Palette,
    /// Which land-cover classes are tinted, the rest shading as grayscale.
    pub cover_enabled: CoverMask,
    /// Window geometry.
    pub window: Window,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            center_lat: DEFAULT_LAT,
            center_lon: DEFAULT_LON,
            ppd: 700.0,
            z_factor: 1.0,
            sun_az_deg: 315.0,
            sun_elev_deg: 45.0,
            coloring: Coloring::Grayscale,
            worldcover_h3db: None,
            dirs: Vec::new(),
            cover_colors: Palette::default(),
            cover_enabled: CoverMask::default(),
            window: Window::default(),
        }
    }
}

impl Config {
    /// Loads config from disk, falling back to defaults on any error.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        println!("Loading config from {path}");
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Writes config to disk, creating the config dir if needed.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory or file cannot be written.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text =
            toml::to_string_pretty(self).map_err(|e| std::io::Error::other(e.to_string()))?;
        std::fs::write(path, text)
    }

    /// Path to `demmit.toml` in the OS config dir.
    pub fn path() -> Option<Utf8PathBuf> {
        let dirs = ProjectDirs::from("xyz", "7r", "demmit")?;
        Utf8PathBuf::from_path_buf(dirs.config_dir().join("demmit.toml")).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::{Coloring, Config, Window};
    use camino::Utf8PathBuf;
    use demmit::{Rgb8, WorldCover};

    #[test]
    fn round_trips_defaults() {
        let cfg = Config::default();
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.center_lat, cfg.center_lat);
        assert_eq!(back.sun_az_deg, cfg.sun_az_deg);
        assert_eq!(back.coloring, Coloring::Grayscale);
    }

    #[test]
    fn round_trips_full() {
        let mut cfg = Config {
            worldcover_h3db: Some(Utf8PathBuf::from("/tmp/wc.h3tree")),
            dirs: vec![Utf8PathBuf::from("/a"), Utf8PathBuf::from("/b")],
            coloring: Coloring::Worldcover,
            window: Window {
                x: Some(10.0),
                y: Some(20.0),
                ..Default::default()
            },
            ..Default::default()
        };
        cfg.cover_colors
            .set(WorldCover::Tree, Rgb8::new(0x01, 0x02, 0x03));
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.dirs, cfg.dirs);
        assert_eq!(back.worldcover_h3db, cfg.worldcover_h3db);
        assert_eq!(back.coloring, Coloring::Worldcover);
        assert_eq!(back.window.x, Some(10.0));
        assert_eq!(back.cover_colors, cfg.cover_colors);
    }

    /// A config written before land-cover colors existed must still load.
    #[test]
    fn loads_config_without_cover_colors() {
        let cfg: Config = toml::from_str("center_lat = 1.0\ncenter_lon = 2.0\n").unwrap();
        assert_eq!(cfg.cover_colors, Config::default().cover_colors);
        assert_eq!(cfg.cover_enabled, Config::default().cover_enabled);
    }

    #[test]
    fn round_trips_disabled_classes() {
        let mut cfg = Config::default();
        cfg.cover_enabled.set(WorldCover::Built, false);
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert!(!back.cover_enabled.get(WorldCover::Built));
        assert!(back.cover_enabled.get(WorldCover::Tree));
    }
}
