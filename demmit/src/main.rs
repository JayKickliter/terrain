use camino::Utf8PathBuf;
use clap::{Args, Parser, Subcommand, ValueEnum};
use demmit::{
    apply_shading, matrix_to_grayscale, matrix_to_wc_image, tile_to_matrix,
    tile_to_worldcover_matrix,
};
use hextree::disktree::DiskTreeMap;
use image::imageops::{resize, FilterType};
use nasadem::Tile;

mod app;
mod config;
mod sun;
mod telemetry;
mod tiles;
mod viewport;
mod worker;

type AnyRes = anyhow::Result<()>;

/// A NASADEM/SRTM '.hgt' file multitool.
#[derive(Clone, Parser)]
struct Cli {
    #[command(subcommand)]
    command: SubCmd,
}

#[derive(Clone, Subcommand)]
enum SubCmd {
    /// Render a NASADEM/SRTM '.hgt' file as an image.
    Render(RenderArgs),

    /// Open the interactive hillshade tile viewer.
    View(ViewArgs),
}

#[derive(Clone, Args)]
struct ViewArgs {
    /// Directory of '.hgt' tiles. Repeat or pass several for multiple resolutions.
    #[clap(long, short, num_args = 1..)]
    dem: Vec<Utf8PathBuf>,

    /// Override the saved/default map center latitude.
    #[clap(long, allow_hyphen_values = true)]
    lat: Option<f64>,

    /// Override the saved/default map center longitude.
    #[clap(long, allow_hyphen_values = true)]
    lon: Option<f64>,
}

#[derive(Clone, Args)]
struct RenderArgs {
    #[clap(long, short, default_value_t = 315.0)]
    azimuth: f32,

    #[clap(long, short, default_value_t = 45.0)]
    elevation: f32,

    /// Resize output to this this value in both x and y dimensions.
    #[clap(long, short)]
    constrain: Option<u32>,

    /// Path to worldcover `h3db`.
    #[clap(long, short)]
    worldcover: Option<Utf8PathBuf>,

    /// Source NASADEM/SRTM hgt file.
    src: Utf8PathBuf,

    /// Optional output file name.
    ///
    /// Image format will be based on `dest`'s extension.
    ///
    /// If not specified, a png will be written with the tile's
    /// basename in the tile's dir.
    dest: Option<Utf8PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
enum BitDepth {
    _8,
    _16,
}

fn render(
    RenderArgs {
        azimuth,
        elevation,
        constrain,
        worldcover,
        src,
        dest,
    }: RenderArgs,
) -> AnyRes {
    let tile = Tile::load(&src)?;
    let out = dest.map_or_else(
        || {
            let mut out = src.clone();
            out.set_extension("png");
            out
        },
        |mut out| {
            if out.is_dir() {
                let name = src.file_name().expect("we already know src is a file");
                out.push(name);
                out.set_extension("png");
            }
            out
        },
    );

    let mat = tile_to_matrix(&tile);
    let cell_size = tile.resolution() as f32 * demmit::METERS_PER_ARCSEC;
    let shaded_mat = apply_shading(
        azimuth.to_radians(),
        elevation.to_radians(),
        cell_size,
        &mat,
    );

    match worldcover {
        Some(wc_path) => {
            let h3db = DiskTreeMap::open(wc_path)?;
            let worldcover_mat = tile_to_worldcover_matrix(&h3db, &tile);
            let mut img = matrix_to_wc_image(&worldcover_mat, &shaded_mat);
            if let Some(size) = constrain {
                img = resize(&img, size, size, FilterType::Lanczos3);
            }
            img.save(out)?;
        }
        None => {
            let mut img = matrix_to_grayscale(&shaded_mat);
            if let Some(size) = constrain {
                img = resize(&img, size, size, FilterType::Lanczos3);
            }
            img.save(out)?;
        }
    }

    Ok(())
}

fn view(args: ViewArgs) -> AnyRes {
    let mut cfg = config::Config::load();
    if !args.dem.is_empty() {
        cfg.dirs = args.dem;
    }
    if cfg.dirs.is_empty() {
        cfg.dirs = vec![Utf8PathBuf::from("data/nasadem/1arcsecond")];
    }
    if let Some(lat) = args.lat {
        cfg.center_lat = lat;
    }
    if let Some(lon) = args.lon {
        cfg.center_lon = lon;
    }
    let _log_guard = telemetry::init();
    app::run(cfg)
}

fn main() -> AnyRes {
    let cli = Cli::parse();
    match cli.command {
        SubCmd::Render(args) => render(args),
        SubCmd::View(args) => view(args),
    }
}
