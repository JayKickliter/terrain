//! Interactive egui hillshade tile viewer.

use crate::{
    config::{Coloring, Config, Window},
    sun::{SunDial, CARDINALS},
    tiles::TileKey,
    viewport::Viewport,
    worker::{self, Pipeline, ShadeParams, Sig, TileUpdate},
};
use anyhow::anyhow;
use camino::{Utf8Path, Utf8PathBuf};
use demmit::{worldcover_at, Sun, WorldCover};
use eframe::egui;
use egui_plot::{Legend, Line, Plot, PlotPoints};
use hextree::disktree::DiskTreeMap;
use nasadem::Tile;
use std::collections::{HashMap, HashSet};
use terrain::{geo::Coord, Profile, TileMode, Tiles};

/// Gradient cache size in the compute thread.
const CACHE_TILES: usize = 48;

/// Largest shaded texture edge, in pixels.
const MAX_TILE_PX: usize = 2048;

/// Tiles beyond the view edge to prefetch on each side.
const PREFETCH_MARGIN: i32 = 1;

/// Minimum seconds between periodic config saves.
const SAVE_THROTTLE_SECS: f64 = 2.0;

/// Sun dial radius in pixels.
const DIAL_RADIUS: f32 = 60.0;

/// Sun dial inset from the map corner in pixels.
const DIAL_MARGIN: f32 = 16.0;

/// Extra grab distance around the dial in pixels.
const DIAL_GRAB_SLOP: f32 = 8.0;

/// Default sun azimuth in degrees (NW).
const DEFAULT_SUN_AZ_DEG: f32 = 315.0;

/// Default sun elevation in degrees.
const DEFAULT_SUN_ELEV_DEG: f32 = 45.0;

/// A tile directory paired with its detected resolution.
struct DirRes {
    path: Utf8PathBuf,
    res: u8,
}

/// Native sample count across a tile at the given resolution.
fn native_px(res: u8) -> usize {
    3600 / usize::from(res.max(1)) + 1
}

/// Detects a directory's tile resolution from its first `.hgt` file.
fn detect_res(dir: &Utf8Path) -> Option<u8> {
    let entry = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "hgt"))?;
    Tile::memmap(&entry).ok().map(|t| t.resolution())
}

/// Index of the coarsest dir whose native resolution still covers `tile_px`.
///
/// `dirs` must be sorted finest resolution first. Falls back to the
/// finest dir when even it cannot cover `tile_px`.
fn pick_dir_index(dirs: &[DirRes], tile_px: usize) -> usize {
    (0..dirs.len())
        .rev()
        .find(|&i| native_px(dirs[i].res) >= tile_px)
        .unwrap_or(0)
}

/// Builds resolution-tagged dirs, finest resolution first.
fn resolve_dirs(paths: &[Utf8PathBuf]) -> Vec<DirRes> {
    let mut dirs: Vec<DirRes> = paths
        .iter()
        .map(|p| DirRes {
            path: p.clone(),
            res: detect_res(p).unwrap_or(1),
        })
        .collect();
    dirs.sort_by_key(|d| d.res);
    dirs
}

/// Launches the native viewer window.
///
/// # Errors
///
/// Returns an error if the window backend fails to start.
pub fn run(cfg: Config) -> crate::AnyRes {
    let mut builder =
        egui::ViewportBuilder::default().with_inner_size([cfg.window.width, cfg.window.height]);
    if let (Some(x), Some(y)) = (cfg.window.x, cfg.window.y) {
        builder = builder.with_position([x, y]);
    }
    let options = eframe::NativeOptions {
        viewport: builder,
        ..Default::default()
    };
    eframe::run_native(
        "demmit",
        options,
        Box::new(move |cc| {
            let app = App::new(cc, cfg)?;
            Ok(Box::new(app) as Box<dyn eframe::App>)
        }),
    )
    .map_err(|e| anyhow!("eframe failed: {e}"))
}

struct App {
    dirs: Vec<DirRes>,
    viewport: Viewport,
    sun: Sun,
    z_factor: f32,
    coloring: Coloring,
    worldcover_h3db: Option<Utf8PathBuf>,
    window: Window,
    coords_input: String,
    pipeline: Pipeline,
    textures: HashMap<TileKey, (Sig, egui::TextureHandle)>,
    inflight: HashMap<TileKey, Sig>,
    missing: HashSet<TileKey>,
    sun_drag: bool,
    last_saved: Config,
    last_save_time: f64,
    path_mode: bool,
    path_start: Option<Coord>,
    path_ends: Option<(Coord, Coord)>,
    profile: Option<Profile>,
    profile_hover: Option<usize>,
    readout: Option<Coord>,
    profile_tiles: Option<Tiles>,
    profile_tiles_dir: Option<Utf8PathBuf>,
    hover_h3db: Option<DiskTreeMap>,
    hover_h3db_path: Option<Utf8PathBuf>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, cfg: Config) -> anyhow::Result<Self> {
        if cfg.dirs.is_empty() {
            return Err(anyhow!("no tile directory configured"));
        }
        let pipeline = worker::spawn(
            cc.egui_ctx.clone(),
            CACHE_TILES,
            cfg.worldcover_h3db.clone(),
        );
        let last_saved = cfg.clone();
        Ok(Self {
            dirs: resolve_dirs(&cfg.dirs),
            viewport: Viewport {
                center_lon: cfg.center_lon,
                center_lat: cfg.center_lat,
                ppd: cfg.ppd,
            },
            sun: Sun {
                az_rad: cfg.sun_az_deg.to_radians(),
                elev_rad: cfg.sun_elev_deg.to_radians(),
            },
            z_factor: cfg.z_factor,
            coloring: cfg.coloring,
            worldcover_h3db: cfg.worldcover_h3db,
            window: cfg.window,
            coords_input: format!("{:.4}, {:.4}", cfg.center_lat, cfg.center_lon),
            pipeline,
            textures: HashMap::new(),
            inflight: HashMap::new(),
            missing: HashSet::new(),
            sun_drag: false,
            last_saved,
            last_save_time: 0.0,
            path_mode: false,
            path_start: None,
            path_ends: None,
            profile: None,
            profile_hover: None,
            readout: None,
            profile_tiles: None,
            profile_tiles_dir: None,
            hover_h3db: None,
            hover_h3db_path: None,
        })
    }

    /// Opens (or reopens) the read-only h3db used for hover lookups.
    fn ensure_hover_h3db(&mut self) {
        if self.worldcover_h3db == self.hover_h3db_path {
            return;
        }
        self.hover_h3db = self
            .worldcover_h3db
            .as_ref()
            .and_then(|p| DiskTreeMap::open(p).ok());
        self.hover_h3db_path = self.worldcover_h3db.clone();
    }

    /// Land-cover class at a geographic point, if worldcover is loaded.
    fn cover_at(&mut self, lat: f64, lon: f64) -> Option<WorldCover> {
        self.ensure_hover_h3db();
        self.hover_h3db
            .as_ref()
            .map(|db| worldcover_at(db, lat, lon))
    }

    /// DEM elevation in meters at a geographic point, if a tile covers it.
    fn elev_at(&mut self, lat: f64, lon: f64) -> Option<i16> {
        self.ensure_profile_tiles();
        let tiles = self.profile_tiles.as_ref()?;
        let c = Coord { x: lon, y: lat };
        tiles.get(c).ok()?.get(c)
    }

    /// Google Maps URL for a point at the current zoom.
    ///
    /// Maps span 360 degrees over `256 * 2^z` pixels, so the viewport's
    /// pixels-per-degree maps back to the `z` zoom level.
    fn gmaps_url(&self, lat: f64, lon: f64) -> String {
        let zoom = (self.viewport.ppd * 360.0 / 256.0).log2().clamp(0.0, 21.0);
        format!("https://www.google.com/maps/@{lat:.6},{lon:.6},{zoom:.1}z")
    }

    /// Ensures a profile tile source built from the finest dir.
    fn ensure_profile_tiles(&mut self) {
        let Some(dir) = self.dirs.first().map(|d| d.path.clone()) else {
            return;
        };
        if self.profile_tiles_dir.as_ref() == Some(&dir) && self.profile_tiles.is_some() {
            return;
        }
        match Tiles::new(dir.clone().into_std_path_buf(), TileMode::MemMap) {
            Ok(tiles) => self.profile_tiles = Some(tiles),
            Err(e) => {
                tracing::warn!(error = ?e, "cannot open tile source for profiles");
                self.profile_tiles = None;
            }
        }
        self.profile_tiles_dir = Some(dir);
    }

    /// Builds and stores a terrain profile between two geographic points.
    fn compute_profile(&mut self, start: Coord, end: Coord) {
        self.ensure_profile_tiles();
        let Some(tiles) = &self.profile_tiles else {
            return;
        };
        let res = self.dirs.first().map_or(1, |d| d.res);
        let step_m = f32::from(res) * demmit::METERS_PER_ARCSEC;
        let s = Coord {
            x: start.x as f32,
            y: start.y as f32,
        };
        let e = Coord {
            x: end.x as f32,
            y: end.y as f32,
        };
        match Profile::builder()
            .start(s)
            .max_step(step_m)
            .end(e)
            .build(tiles)
        {
            Ok(profile) => {
                tracing::info!(points = profile.distances_m.len(), "built terrain profile");
                self.profile = Some(profile);
            }
            Err(e) => {
                tracing::warn!(error = ?e, "profile build failed");
                self.profile = None;
            }
        }
    }

    /// Writes config now if it changed since the last save.
    fn persist(&mut self) {
        let current = self.to_config();
        if current != self.last_saved {
            let _ = current.save();
            self.last_saved = current;
        }
    }

    /// Persists at most once per [`SAVE_THROTTLE_SECS`].
    ///
    /// winit terminates the process on window close without running
    /// destructors, so state must be saved during the run, not on exit.
    fn maybe_persist(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if now - self.last_save_time > SAVE_THROTTLE_SECS {
            self.persist();
            self.last_save_time = now;
        }
    }

    fn respawn(&mut self, ctx: &egui::Context) {
        self.pipeline = worker::spawn(ctx.clone(), CACHE_TILES, self.worldcover_h3db.clone());
        self.textures.clear();
        self.inflight.clear();
        self.missing.clear();
    }

    fn to_config(&self) -> Config {
        Config {
            center_lat: self.viewport.center_lat,
            center_lon: self.viewport.center_lon,
            ppd: self.viewport.ppd,
            z_factor: self.z_factor,
            sun_az_deg: self.sun.az_rad.to_degrees(),
            sun_elev_deg: self.sun.elev_rad.to_degrees(),
            coloring: self.coloring,
            worldcover_h3db: self.worldcover_h3db.clone(),
            dirs: self.dirs.iter().map(|d| d.path.clone()).collect(),
            window: self.window,
        }
    }

    fn dial(&self, area: egui::Rect) -> SunDial {
        SunDial {
            center: egui::pos2(
                area.right() - DIAL_MARGIN - DIAL_RADIUS,
                area.top() + DIAL_MARGIN + DIAL_RADIUS,
            ),
            radius: DIAL_RADIUS,
        }
    }

    fn capture_window(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            let vp = i.viewport();
            if let Some(r) = vp.inner_rect {
                self.window.width = r.width();
                self.window.height = r.height();
            }
            if let Some(r) = vp.outer_rect {
                self.window.x = Some(r.min.x);
                self.window.y = Some(r.min.y);
            }
        });
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let resp =
                ui.add(egui::TextEdit::singleline(&mut self.coords_input).desired_width(150.0));
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                if let Some((lat, lon)) = parse_latlon(&self.coords_input) {
                    self.viewport.center_lat = lat;
                    self.viewport.center_lon = lon;
                }
            }
            if !resp.has_focus() {
                self.coords_input = format!(
                    "{:.4}, {:.4}",
                    self.viewport.center_lat, self.viewport.center_lon
                );
            }
            ui.separator();
            ui.add(egui::Slider::new(&mut self.z_factor, 1.0..=10.0).text("z"));
            ui.separator();
            if ui
                .button("1:1")
                .on_hover_text("one screen pixel per DEM sample")
                .clicked()
            {
                let res = self.dirs.first().map_or(1, |d| d.res);
                self.viewport.ppd = 3600.0 / f64::from(res);
            }
            ui.separator();

            let has_cover = self.worldcover_h3db.is_some();
            let mut cover_on = self.coloring == Coloring::Worldcover;
            if ui
                .add_enabled(has_cover, egui::Checkbox::new(&mut cover_on, "land cover"))
                .changed()
            {
                self.coloring = if cover_on {
                    Coloring::Worldcover
                } else {
                    Coloring::Grayscale
                };
            }

            if ui.button("tiles…").clicked() {
                if let Some(dir) = rfd::FileDialog::new()
                    .pick_folder()
                    .and_then(|p| Utf8PathBuf::from_path_buf(p).ok())
                {
                    let mut paths: Vec<Utf8PathBuf> =
                        self.dirs.iter().map(|d| d.path.clone()).collect();
                    if !paths.contains(&dir) {
                        paths.push(dir);
                    }
                    self.dirs = resolve_dirs(&paths);
                    let ctx = ui.ctx().clone();
                    self.respawn(&ctx);
                    self.persist();
                }
            }
            if ui.button("cover…").clicked() {
                if let Some(file) = rfd::FileDialog::new()
                    .pick_file()
                    .and_then(|p| Utf8PathBuf::from_path_buf(p).ok())
                {
                    self.worldcover_h3db = Some(file);
                    self.coloring = Coloring::Worldcover;
                    let ctx = ui.ctx().clone();
                    self.respawn(&ctx);
                    self.persist();
                }
            }
            ui.separator();
            ui.toggle_value(&mut self.path_mode, "path")
                .on_hover_text("click two points to plot a terrain profile");

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button("🗺")
                    .on_hover_text("open current view in Google Maps")
                    .clicked()
                {
                    let url =
                        self.gmaps_url(self.viewport.center_lat, self.viewport.center_lon);
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
            });
        });
    }

    fn handle_input(
        &mut self,
        ui: &egui::Ui,
        area: egui::Rect,
        proj: egui::Rect,
        response: &egui::Response,
    ) {
        let dial = self.dial(area);

        if response.secondary_clicked() {
            if let Some(p) = response.interact_pointer_pos() {
                let (lon, lat) = self.viewport.screen_to_world(proj, p);
                self.readout = Some(Coord { x: lon, y: lat });
            }
        }

        if response.double_clicked()
            && response
                .interact_pointer_pos()
                .is_some_and(|p| dial.contains(p, DIAL_GRAB_SLOP))
        {
            self.sun = Sun {
                az_rad: DEFAULT_SUN_AZ_DEG.to_radians(),
                elev_rad: DEFAULT_SUN_ELEV_DEG.to_radians(),
            };
            return;
        }

        if self.path_mode && response.clicked() {
            if let Some(p) = response.interact_pointer_pos() {
                let (lon, lat) = self.viewport.screen_to_world(proj, p);
                let point = Coord { x: lon, y: lat };
                match self.path_start.take() {
                    None => {
                        self.path_ends = None;
                        self.profile = None;
                        self.path_start = Some(point);
                    }
                    Some(start) => {
                        self.path_ends = Some((start, point));
                        self.compute_profile(start, point);
                    }
                }
            }
            return;
        }

        if response.drag_started() {
            self.sun_drag = response
                .interact_pointer_pos()
                .is_some_and(|p| dial.contains(p, DIAL_GRAB_SLOP));
        }
        if response.dragged() {
            if self.sun_drag {
                if let Some(p) = response.interact_pointer_pos() {
                    self.sun = dial.sun_from_pos(p);
                }
            } else {
                let d = response.drag_delta();
                self.viewport.pan_pixels(d.x, d.y);
            }
        }
        if response.drag_stopped() {
            self.sun_drag = false;
        }

        if !self.sun_drag {
            let anchor = response.hover_pos().unwrap_or_else(|| proj.center());
            let (scroll, zoom_delta) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            if scroll != 0.0 {
                self.viewport
                    .zoom_about(proj, anchor, (f64::from(scroll) * 0.0015).exp());
            }
            if (zoom_delta - 1.0).abs() > f32::EPSILON {
                self.viewport
                    .zoom_about(proj, anchor, f64::from(zoom_delta));
            }
        }
    }

    fn draw_map(&mut self, ui: &egui::Ui, clip: egui::Rect, proj: egui::Rect) {
        let tile_px = (self.viewport.ppd.round() as usize).clamp(64, MAX_TILE_PX);
        let params = ShadeParams {
            sun: self.sun,
            z_factor: self.z_factor,
            tile_px,
            coloring: self.coloring,
        };
        let want_sig = Sig::new(params);

        for update in self.pipeline.drain() {
            match update {
                TileUpdate::Shaded {
                    key,
                    sig,
                    width,
                    height,
                    rgba,
                } => {
                    let image = egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba);
                    let tex =
                        ui.ctx()
                            .load_texture(key.filename(), image, egui::TextureOptions::LINEAR);
                    self.textures.insert(key, (sig, tex));
                    if self.inflight.get(&key) == Some(&sig) {
                        self.inflight.remove(&key);
                    }
                }
                TileUpdate::Missing { key } => {
                    self.inflight.remove(&key);
                    self.missing.insert(key);
                }
                TileUpdate::Cancelled { key, sig } => {
                    if self.inflight.get(&key) == Some(&sig) {
                        self.inflight.remove(&key);
                    }
                }
            }
        }

        let mut visible = self.viewport.visible_tiles(proj, PREFETCH_MARGIN);
        // Shuffle so tiles pop in randomly rather than row by row.
        fastrand::shuffle(&mut visible);

        // Resolve each tile to the best available source: the ideal
        // resolution for this zoom, falling back to other dirs when that
        // tile is known missing there.
        let resolved: Vec<(i32, i32, TileKey, Utf8PathBuf)> = visible
            .iter()
            .filter_map(|&(lon, lat)| self.resolve_tile(lon, lat, tile_px))
            .collect();

        // Publish what this frame needs before requesting, so the
        // compute thread can cancel work left over from a prior view.
        let wanted: HashMap<TileKey, Sig> =
            resolved.iter().map(|(_, _, k, _)| (*k, want_sig)).collect();
        self.pipeline.publish_wanted(wanted);

        let mut keep: HashSet<TileKey> = HashSet::new();
        let painter = ui.painter_at(clip);
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));

        for (lon, lat, key, dir_path) in resolved {
            keep.insert(key);
            let have = self
                .textures
                .get(&key)
                .is_some_and(|(sig, _)| *sig == want_sig);
            let asked = self.inflight.get(&key) == Some(&want_sig);
            if !have && !asked {
                self.pipeline.want(key, key.path(&dir_path), params);
                self.inflight.insert(key, want_sig);
            }
            if let Some((_, tex)) = self.textures.get(&key) {
                let rect = self.viewport.tile_rect(proj, lon, lat);
                painter.image(tex.id(), rect, uv, egui::Color32::WHITE);
            }
        }

        self.textures.retain(|k, _| keep.contains(k));
        self.inflight.retain(|k, _| keep.contains(k));
    }

    /// Picks the source dir for a tile: ideal resolution first, then any
    /// other dir whose copy of the tile is not known missing.
    ///
    /// Returns `None` when every dir is missing this tile.
    fn resolve_tile(
        &self,
        lon: i32,
        lat: i32,
        tile_px: usize,
    ) -> Option<(i32, i32, TileKey, Utf8PathBuf)> {
        let ideal = pick_dir_index(&self.dirs, tile_px);
        let order = std::iter::once(ideal).chain((0..self.dirs.len()).filter(|&i| i != ideal));
        for i in order {
            let key = TileKey {
                lat,
                lon,
                res: self.dirs[i].res,
            };
            if !self.missing.contains(&key) {
                return Some((lon, lat, key, self.dirs[i].path.clone()));
            }
        }
        None
    }

    fn draw_sun(&self, ui: &egui::Ui, area: egui::Rect) {
        let dial = self.dial(area);
        let p = ui.painter_at(area);
        p.circle_filled(
            dial.center,
            dial.radius,
            egui::Color32::from_black_alpha(120),
        );
        p.circle_stroke(
            dial.center,
            dial.radius,
            egui::Stroke::new(1.5, egui::Color32::GRAY),
        );
        for (ang, label) in CARDINALS {
            let lp = egui::pos2(
                dial.center.x + (dial.radius + 9.0) * ang.sin(),
                dial.center.y - (dial.radius + 9.0) * ang.cos(),
            );
            p.text(
                lp,
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(11.0),
                egui::Color32::GRAY,
            );
        }
        let handle = dial.handle_pos(self.sun);
        p.line_segment(
            [dial.center, handle],
            egui::Stroke::new(1.5, egui::Color32::from_rgb(255, 210, 60)),
        );
        p.circle_filled(handle, 5.0, egui::Color32::from_rgb(255, 210, 60));
        p.text(
            egui::pos2(dial.center.x, dial.center.y + dial.radius + 12.0),
            egui::Align2::CENTER_TOP,
            format!(
                "az {:.0}  el {:.0}",
                self.sun.az_rad.to_degrees(),
                self.sun.elev_rad.to_degrees()
            ),
            egui::FontId::proportional(11.0),
            egui::Color32::LIGHT_GRAY,
        );
    }

    fn draw_path(&self, ui: &egui::Ui, clip: egui::Rect, proj: egui::Rect) {
        let painter = ui.painter_at(clip);
        let color = egui::Color32::from_rgb(255, 80, 80);
        if let Some((s, e)) = self.path_ends {
            let ps = self.viewport.world_to_screen(proj, s.x, s.y);
            let pe = self.viewport.world_to_screen(proj, e.x, e.y);
            let stroke = egui::Stroke::new(2.0, color);
            match &self.profile {
                Some(p) if p.great_circle.len() >= 2 => {
                    let pts = p
                        .great_circle
                        .iter()
                        .map(|c| {
                            self.viewport
                                .world_to_screen(proj, f64::from(c.x()), f64::from(c.y()))
                        })
                        .collect();
                    painter.add(egui::Shape::line(pts, stroke));
                }
                _ => {
                    painter.line_segment([ps, pe], stroke);
                }
            }
            painter.circle_filled(ps, 4.0, color);
            painter.circle_filled(pe, 4.0, color);
            if let Some(pt) = self
                .profile_hover
                .and_then(|i| self.profile.as_ref()?.great_circle.get(i))
            {
                let hp = self
                    .viewport
                    .world_to_screen(proj, f64::from(pt.x()), f64::from(pt.y()));
                painter.circle_filled(hp, 6.0, egui::Color32::WHITE);
                painter.circle_stroke(hp, 6.0, egui::Stroke::new(2.0, color));
            }
        } else if let Some(s) = self.path_start {
            let ps = self.viewport.world_to_screen(proj, s.x, s.y);
            painter.circle_filled(ps, 4.0, color);
        }
    }

    /// Draws the right-click readout box at its geographic anchor.
    ///
    /// Lines: lat/lon, land-cover class, DEM elevation. Text is selectable
    /// and copyable.
    fn draw_readout(&mut self, ui: &egui::Ui, clip: egui::Rect, proj: egui::Rect) {
        let Some(pt) = self.readout else {
            return;
        };
        let (lat, lon) = (pt.y, pt.x);
        let mut lines = vec![format!("{lat:.5}, {lon:.5}")];
        if let Some(cover) = self.cover_at(lat, lon) {
            lines.push(cover.to_string());
        }
        if let Some(elev) = self.elev_at(lat, lon) {
            lines.push(format!("{elev} m"));
        }
        let text = lines.join("\n");
        let gmaps_url = self.gmaps_url(lat, lon);

        let painter = ui.painter_at(clip);
        let anchor = self.viewport.world_to_screen(proj, lon, lat);
        let dot = egui::Color32::from_rgb(255, 220, 80);
        painter.circle_filled(anchor, 3.5, dot);
        painter.circle_stroke(anchor, 3.5, egui::Stroke::new(1.0, egui::Color32::BLACK));

        let mut dismiss = false;
        egui::Area::new(egui::Id::new("readout_box"))
            .order(egui::Order::Foreground)
            .fixed_pos(anchor + egui::vec2(10.0, 10.0))
            .constrain_to(clip)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.small_button("copy").clicked() {
                            ui.ctx().copy_text(text.clone());
                        }
                        if ui.small_button("maps").clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(&gmaps_url));
                        }
                        if ui.small_button("✕").clicked() {
                            dismiss = true;
                        }
                    });
                    ui.add(
                        egui::Label::new(egui::RichText::new(&text).monospace()).selectable(true),
                    );
                });
            });
        if dismiss {
            self.readout = None;
        }
    }

    fn draw_profile_panel(&mut self, ui: &mut egui::Ui) {
        let Some((terrain_pts, los_pts, floor_m, total_km)) = self.profile.as_ref().map(|p| {
            let terrain: PlotPoints = p
                .distances_m
                .iter()
                .zip(p.terrain_elev_m.iter())
                .map(|(&d, &e)| [f64::from(d) / 1000.0, f64::from(e)])
                .collect();
            let los: PlotPoints = p
                .distances_m
                .iter()
                .zip(p.los_elev_m.iter())
                .map(|(&d, &e)| [f64::from(d) / 1000.0, f64::from(e)])
                .collect();
            let floor = p
                .terrain_elev_m
                .iter()
                .copied()
                .fold(f32::INFINITY, f32::min);
            let total_km = p.distances_m.last().copied().unwrap_or(0.0) / 1000.0;
            (terrain, los, floor, total_km)
        }) else {
            return;
        };

        ui.horizontal(|ui| {
            ui.label(format!("terrain profile — {total_km:.1} km"));
            if ui.button("clear").clicked() {
                self.profile = None;
                self.path_ends = None;
                self.path_start = None;
            }
        });

        let hover = Plot::new("terrain_profile")
            .legend(Legend::default())
            .x_axis_label("km")
            .y_axis_label("m")
            .show(ui, |plot_ui| {
                plot_ui.line(
                    Line::new("terrain", terrain_pts)
                        .fill(floor_m)
                        .color(egui::Color32::from_rgb(150, 110, 70)),
                );
                plot_ui.line(
                    Line::new("line of sight", los_pts)
                        .color(egui::Color32::from_rgb(230, 180, 50)),
                );
                plot_ui.pointer_coordinate()
            })
            .inner;
        self.profile_hover =
            hover.and_then(|pt| self.profile_index_at_km(pt.x, f64::from(total_km)));
    }

    /// Nearest great-circle sample index to a plot x-coordinate in km.
    ///
    /// Returns `None` when the coordinate lies outside the profile.
    fn profile_index_at_km(&self, km: f64, total_km: f64) -> Option<usize> {
        if !(0.0..=total_km).contains(&km) {
            return None;
        }
        let p = self.profile.as_ref()?;
        let target_m = km * 1000.0;
        p.distances_m
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let da = (f64::from(**a) - target_m).abs();
                let db = (f64::from(**b) - target_m).abs();
                da.total_cmp(&db)
            })
            .map(|(i, _)| i)
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.capture_window(ui.ctx());
        if self.profile.is_some() {
            egui::Panel::bottom("terrain_profile_panel")
                .resizable(true)
                .default_size(200.0)
                .show(ui, |ui| self.draw_profile_panel(ui));
        }
        egui::CentralPanel::default().show(ui, |ui| {
            self.controls(ui);
            let (area, response) =
                ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
            // Project against the full window height so the bottom panel
            // covers the map rather than shifting tiles when it opens.
            let screen_bottom = ui.ctx().input(|i| i.content_rect().bottom());
            let proj = egui::Rect::from_min_max(
                area.min,
                egui::pos2(area.max.x, screen_bottom.max(area.max.y)),
            );
            self.handle_input(ui, area, proj, &response);
            self.draw_map(ui, area, proj);
            self.draw_path(ui, area, proj);
            self.draw_sun(ui, area);
            self.draw_readout(ui, area, proj);
        });
        self.maybe_persist(ui.ctx());
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.persist();
    }
}

/// Parses `"lat, lon"` or `"lat lon"` decimal degrees.
fn parse_latlon(s: &str) -> Option<(f64, f64)> {
    let parts: Vec<&str> = s
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() != 2 {
        return None;
    }
    let lat = parts[0].parse::<f64>().ok()?;
    let lon = parts[1].parse::<f64>().ok()?;
    ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon)).then_some((lat, lon))
}

#[cfg(test)]
mod tests {
    use super::{native_px, parse_latlon, pick_dir_index, DirRes};
    use camino::Utf8PathBuf;

    fn dirs() -> Vec<DirRes> {
        vec![
            DirRes {
                path: Utf8PathBuf::from("fine"),
                res: 1,
            },
            DirRes {
                path: Utf8PathBuf::from("coarse"),
                res: 3,
            },
        ]
    }

    #[test]
    fn native_px_matches_nasadem() {
        assert_eq!(native_px(1), 3601);
        assert_eq!(native_px(3), 1201);
    }

    #[test]
    fn lod_uses_coarse_when_zoomed_out() {
        assert_eq!(pick_dir_index(&dirs(), 500), 1);
        assert_eq!(pick_dir_index(&dirs(), 1201), 1);
    }

    #[test]
    fn lod_uses_fine_when_zoomed_in() {
        assert_eq!(pick_dir_index(&dirs(), 1500), 0);
        assert_eq!(pick_dir_index(&dirs(), 3601), 0);
        assert_eq!(pick_dir_index(&dirs(), 9000), 0);
    }

    #[test]
    fn parses_comma_and_space() {
        assert_eq!(parse_latlon("38.84, -105.04"), Some((38.84, -105.04)));
        assert_eq!(parse_latlon("38.84 -105.04"), Some((38.84, -105.04)));
        assert_eq!(parse_latlon("  44.5,-71.5 "), Some((44.5, -71.5)));
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(parse_latlon("hello"), None);
        assert_eq!(parse_latlon("1 2 3"), None);
        assert_eq!(parse_latlon("100, 0"), None);
        assert_eq!(parse_latlon("0, 200"), None);
    }
}
