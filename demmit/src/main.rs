use camino::Utf8PathBuf;
use clap::{Args, Parser, Subcommand, ValueEnum};
use demmit::{apply_shading, matrix_to_wc_image, tile_to_matrix, tile_to_worldcover_matrix};
use hextree::disktree::DiskTreeMap;
use image::imageops::{resize, FilterType};
use nasadem::Tile;

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
    worldcover: Utf8PathBuf,

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
    let shaded_mat = apply_shading(azimuth.to_radians(), elevation.to_radians(), &mat);
    let h3db = DiskTreeMap::open(worldcover).unwrap();
    let worldcover_mat = tile_to_worldcover_matrix(&h3db, &tile);
    let mut img = matrix_to_wc_image(&worldcover_mat, &shaded_mat);
    if let Some(size) = constrain {
        img = resize(&img, size, size, FilterType::Lanczos3);
    }
    img.save(out)?;

    Ok(())
}

fn main() -> AnyRes {
    let cli = Cli::parse();
    match cli.command {
        SubCmd::Render(args) => render(args),
    }
}
