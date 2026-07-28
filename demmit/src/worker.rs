//! Background IO and compute pipeline, keeping file reads and shading off the UI thread.
//!
//! IO thread memmaps tiles; the compute thread downsamples, builds
//! gradients, and shades. The UI only sends requests and uploads
//! finished textures.

use crate::{config::Coloring, tiles::TileKey};
use camino::Utf8PathBuf;
use crossbeam_channel::{unbounded, Receiver, Sender};
use demmit::{tile_to_worldcover_downsampled, Gradients, Palette, Sun, WorldCover};
use dropclock::DropClock;
use eframe::egui;
use hextree::disktree::DiskTreeMap;
use lru::LruCache;
use nasadem::Tile;
use std::{
    collections::HashMap,
    num::NonZeroUsize,
    sync::{Arc, Mutex},
    thread,
};

/// Shared set of tiles the UI currently wants, keyed by signature.
///
/// Compute drops any dequeued work whose `(key, sig)` is absent, so a
/// fast pan or zoom cancels shading that is no longer on screen.
type Wanted = Arc<Mutex<HashMap<TileKey, Sig>>>;

/// Shading parameters that vary per reshade.
#[derive(Clone, Copy)]
pub struct ShadeParams {
    /// Sun position.
    pub sun: Sun,
    /// Vertical exaggeration.
    pub z_factor: f32,
    /// Target texture edge length in pixels.
    pub tile_px: usize,
    /// Coloring mode.
    pub coloring: Coloring,
    /// Per-class land-cover tint colors.
    pub palette: Palette,
}

/// Compact identity of a shading result, used to skip redundant work.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Sig {
    az: u32,
    elev: u32,
    z: u32,
    tile_px: usize,
    coloring: u8,
    palette: Palette,
}

impl Sig {
    /// Derives the signature of a shading request.
    pub fn new(params: ShadeParams) -> Self {
        Self {
            az: params.sun.az_rad.to_bits(),
            elev: params.sun.elev_rad.to_bits(),
            z: params.z_factor.to_bits(),
            tile_px: params.tile_px,
            coloring: match params.coloring {
                Coloring::Grayscale => 0,
                Coloring::Worldcover => 1,
            },
            palette: params.palette,
        }
    }
}

/// A finished result delivered to the UI thread.
pub enum TileUpdate {
    /// A shaded RGBA texture ready to upload.
    Shaded {
        /// Which tile.
        key: TileKey,
        /// Signature the texture was shaded with.
        sig: Sig,
        /// Texture width in pixels.
        width: usize,
        /// Texture height in pixels.
        height: usize,
        /// RGBA pixels, `width * height * 4` bytes.
        rgba: Vec<u8>,
    },
    /// The tile file is absent.
    Missing {
        /// Which tile.
        key: TileKey,
    },
    /// A request was dropped as stale before shading.
    ///
    /// Lets the UI clear its in-flight record and re-request if the
    /// tile is still needed.
    Cancelled {
        /// Which tile.
        key: TileKey,
        /// Signature of the dropped request.
        sig: Sig,
    },
}

enum ToCompute {
    Want {
        key: TileKey,
        path: Utf8PathBuf,
        params: ShadeParams,
    },
    Loaded {
        key: TileKey,
        tile: Box<Tile>,
    },
    Missing {
        key: TileKey,
    },
}

enum ToIo {
    Load { key: TileKey, path: Utf8PathBuf },
}

/// Handle the UI uses to request work and collect results.
pub struct Pipeline {
    to_compute: Sender<ToCompute>,
    updates: Receiver<TileUpdate>,
    wanted: Wanted,
}

impl Pipeline {
    /// Requests a shaded texture for `key` at the given parameters.
    pub fn want(&self, key: TileKey, path: Utf8PathBuf, params: ShadeParams) {
        let _ = self.to_compute.send(ToCompute::Want { key, path, params });
    }

    /// Replaces the set of currently-wanted tiles.
    ///
    /// Publish this before sending the frame's [`Pipeline::want`] calls
    /// so queued-but-stale work is cancelled instead of shaded.
    pub fn publish_wanted(&self, wanted: HashMap<TileKey, Sig>) {
        *self.wanted.lock().expect("wanted mutex") = wanted;
    }

    /// Drains all results ready this frame.
    pub fn drain(&self) -> impl Iterator<Item = TileUpdate> + '_ {
        self.updates.try_iter()
    }
}

/// Spawns the IO and compute threads and returns a [`Pipeline`] handle.
///
/// `cache_tiles` bounds how many tiles' gradients stay resident.
/// `worldcover` is the optional WorldCover h3 disktree path.
pub fn spawn(ctx: egui::Context, cache_tiles: usize, worldcover: Option<Utf8PathBuf>) -> Pipeline {
    let (to_compute, compute_rx) = unbounded::<ToCompute>();
    let (to_io, io_rx) = unbounded::<ToIo>();
    let (update_tx, updates) = unbounded::<TileUpdate>();
    let wanted: Wanted = Arc::new(Mutex::new(HashMap::new()));

    let io_to_compute = to_compute.clone();
    thread::Builder::new()
        .name("demmit-io".into())
        .spawn(move || io_loop(&io_rx, &io_to_compute))
        .expect("spawn io thread");

    let compute_wanted = Arc::clone(&wanted);
    thread::Builder::new()
        .name("demmit-compute".into())
        .spawn(move || {
            compute_loop(
                &compute_rx,
                &to_io,
                &update_tx,
                &ctx,
                cache_tiles,
                worldcover,
                &compute_wanted,
            );
        })
        .expect("spawn compute thread");

    Pipeline {
        to_compute,
        updates,
        wanted,
    }
}

fn io_loop(rx: &Receiver<ToIo>, compute: &Sender<ToCompute>) {
    while let Ok(ToIo::Load { key, path }) = rx.recv() {
        let name = key.filename();
        let _timer = DropClock::new(move |t| {
            tracing::debug!(tile = %name, elapsed = ?t.elapsed(), "memmap");
        });
        let msg = match Tile::memmap(&path) {
            Ok(tile) => ToCompute::Loaded {
                key,
                tile: Box::new(tile),
            },
            Err(e) => {
                tracing::warn!(tile = %key.filename(), path = %path, error = %e, "tile missing");
                ToCompute::Missing { key }
            }
        };
        if compute.send(msg).is_err() {
            break;
        }
    }
}

struct Caches {
    tiles: LruCache<TileKey, Box<Tile>>,
    grads: LruCache<TileKey, (usize, Gradients)>,
    cover: LruCache<TileKey, (usize, Vec<WorldCover>)>,
    h3db: Option<DiskTreeMap>,
}

#[allow(clippy::too_many_arguments)]
fn compute_loop(
    rx: &Receiver<ToCompute>,
    to_io: &Sender<ToIo>,
    update_tx: &Sender<TileUpdate>,
    ctx: &egui::Context,
    cache_tiles: usize,
    worldcover: Option<Utf8PathBuf>,
    wanted: &Wanted,
) {
    let cap = NonZeroUsize::new(cache_tiles.max(1)).expect("nonzero cache");
    let mut caches = Caches {
        tiles: LruCache::new(cap),
        grads: LruCache::new(cap),
        cover: LruCache::new(cap),
        h3db: worldcover.and_then(|p| DiskTreeMap::open(p).ok()),
    };
    let mut pending: HashMap<TileKey, ShadeParams> = HashMap::new();

    while let Ok(msg) = rx.recv() {
        match msg {
            ToCompute::Want { key, path, params } => {
                if !is_wanted(wanted, key, params) {
                    cancel(update_tx, ctx, key, params);
                    continue;
                }
                if caches.tiles.contains(&key) {
                    shade_and_send(key, &mut caches, params, update_tx, ctx);
                } else {
                    let first = !pending.contains_key(&key);
                    pending.insert(key, params);
                    if first {
                        let _ = to_io.send(ToIo::Load { key, path });
                    }
                }
            }
            ToCompute::Loaded { key, tile } => {
                caches.tiles.put(key, tile);
                if let Some(params) = pending.remove(&key) {
                    if is_wanted(wanted, key, params) {
                        shade_and_send(key, &mut caches, params, update_tx, ctx);
                    } else {
                        cancel(update_tx, ctx, key, params);
                    }
                }
            }
            ToCompute::Missing { key } => {
                pending.remove(&key);
                let _ = update_tx.send(TileUpdate::Missing { key });
                ctx.request_repaint();
            }
        }
    }
}

/// Whether the UI still wants `key` shaded at `params`.
fn is_wanted(wanted: &Wanted, key: TileKey, params: ShadeParams) -> bool {
    wanted.lock().expect("wanted mutex").get(&key) == Some(&Sig::new(params))
}

/// Notifies the UI that a stale request was dropped.
fn cancel(update_tx: &Sender<TileUpdate>, ctx: &egui::Context, key: TileKey, params: ShadeParams) {
    let _ = update_tx.send(TileUpdate::Cancelled {
        key,
        sig: Sig::new(params),
    });
    ctx.request_repaint();
}

#[tracing::instrument(skip_all, fields(tile = %key.filename(), px = params.tile_px))]
fn shade_and_send(
    key: TileKey,
    caches: &mut Caches,
    params: ShadeParams,
    update_tx: &Sender<TileUpdate>,
    ctx: &egui::Context,
) {
    let tile = caches.tiles.get(&key).expect("tile present");

    if caches
        .grads
        .peek(&key)
        .is_none_or(|(px, _)| *px != params.tile_px)
    {
        let _timer = DropClock::new(|t| tracing::debug!(elapsed = ?t.elapsed(), "build gradients"));
        let g = Gradients::from_tile_downsampled(tile, params.tile_px);
        caches.grads.put(key, (params.tile_px, g));
    }

    let use_cover = params.coloring == Coloring::Worldcover && caches.h3db.is_some();
    if use_cover
        && caches
            .cover
            .peek(&key)
            .is_none_or(|(px, _)| *px != params.tile_px)
    {
        let _timer =
            DropClock::new(|t| tracing::debug!(elapsed = ?t.elapsed(), "build worldcover"));
        let h3db = caches.h3db.as_ref().expect("checked present");
        let classes = tile_to_worldcover_downsampled(h3db, tile, key.lon, key.lat, params.tile_px);
        caches.cover.put(key, (params.tile_px, classes));
    }

    let (_, g) = caches.grads.get(&key).expect("gradients present");
    let (width, height) = g.dimensions();
    let mut rgba = vec![0_u8; width * height * 4];
    {
        let _timer = DropClock::new(|t| tracing::trace!(elapsed = ?t.elapsed(), "reshade"));
        if use_cover {
            let (_, classes) = caches.cover.get(&key).expect("cover present");
            g.shade_worldcover(
                classes,
                &params.palette,
                params.sun,
                params.z_factor,
                &mut rgba,
            );
        } else {
            g.shade_grayscale(params.sun, params.z_factor, &mut rgba);
        }
    }

    let _ = update_tx.send(TileUpdate::Shaded {
        key,
        sig: Sig::new(params),
        width,
        height,
        rgba,
    });
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::{spawn, ShadeParams, Sig, TileUpdate};
    use crate::{config::Coloring, tiles::TileKey};
    use camino::Utf8PathBuf;
    use demmit::{Palette, Sun};
    use eframe::egui;
    use std::{collections::HashMap, time::Duration};

    fn dir() -> Utf8PathBuf {
        Utf8PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../data/nasadem/3arcsecond"
        ))
    }

    fn params(tile_px: usize) -> ShadeParams {
        ShadeParams {
            sun: Sun {
                az_rad: 315.0_f32.to_radians(),
                elev_rad: 45.0_f32.to_radians(),
            },
            z_factor: 1.0,
            tile_px,
            coloring: Coloring::Grayscale,
            palette: Palette::default(),
        }
    }

    #[test]
    fn shades_real_tile_at_requested_resolution() {
        let pipe = spawn(egui::Context::default(), 8, None);
        let key = TileKey {
            lat: 44,
            lon: -72,
            res: 3,
        };
        let p = params(256);
        pipe.publish_wanted(HashMap::from([(key, Sig::new(p))]));
        pipe.want(key, key.path(&dir()), p);

        let update = pipe
            .updates
            .recv_timeout(Duration::from_secs(20))
            .expect("no update received");
        match update {
            TileUpdate::Shaded {
                width,
                height,
                rgba,
                ..
            } => {
                assert_eq!(width, 256);
                assert_eq!(height, 256);
                assert_eq!(rgba.len(), width * height * 4);
            }
            TileUpdate::Missing { .. } => panic!("real tile reported missing"),
            TileUpdate::Cancelled { .. } => panic!("wanted tile reported cancelled"),
        }
    }

    #[test]
    fn reports_absent_tile_missing() {
        let pipe = spawn(egui::Context::default(), 8, None);
        let key = TileKey {
            lat: 0,
            lon: 0,
            res: 3,
        };
        let p = params(256);
        pipe.publish_wanted(HashMap::from([(key, Sig::new(p))]));
        pipe.want(key, key.path(&dir()), p);

        let update = pipe
            .updates
            .recv_timeout(Duration::from_secs(20))
            .expect("no update received");
        assert!(matches!(update, TileUpdate::Missing { .. }));
    }

    #[test]
    fn cancels_unwanted_request() {
        let pipe = spawn(egui::Context::default(), 8, None);
        let key = TileKey {
            lat: 44,
            lon: -72,
            res: 3,
        };
        // No publish_wanted, so this request is stale on arrival.
        pipe.want(key, key.path(&dir()), params(256));

        let update = pipe
            .updates
            .recv_timeout(Duration::from_secs(5))
            .expect("expected a cancellation");
        assert!(matches!(update, TileUpdate::Cancelled { .. }));
    }
}
