use camino::Utf8PathBuf;
use clap::{Args, Parser, Subcommand, ValueEnum};
use demmit::{shade, matrix_to_image, tile_to_matrix};
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

    #[clap(long, short, default_value_t = 20.0)]
    elevation: f32,

    /// Resize output to this this value in both x and y dimensions.
    #[clap(long, short)]
    constrain: Option<u32>,

    /// Bit depth
    #[clap(long, short)]
    depth: Option<BitDepth>,

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
        depth,
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
    let shaded = shade(azimuth.to_radians(), elevation.to_radians(), &mat);

    match (depth, out.extension()) {
        (None | Some(BitDepth::_8), Some("jpg")) => {
            let mut img = matrix_to_image::<u8>(&shaded);
            if let Some(size) = constrain {
                img = resize(&img, size, size, FilterType::Lanczos3);
            }
            img.save(out)?;
        }
        (None | Some(BitDepth::_16), Some("png" | "tif" | "tiff")) => {
            let mut img = matrix_to_image::<u16>(&shaded);
            if let Some(size) = constrain {
                img = resize(&img, size, size, FilterType::Lanczos3);
            }
            img.save(out)?;
        }
        (Some(BitDepth::_16), _) => {
            let mut img = matrix_to_image::<u16>(&shaded);
            if let Some(size) = constrain {
                img = resize(&img, size, size, FilterType::Lanczos3);
            }

            img.save(out)?;
        }
        (_, _) => {
            let mut img = matrix_to_image::<u8>(&shaded);
            if let Some(size) = constrain {
                img = resize(&img, size, size, FilterType::Lanczos3);
            }
            img.save(out)?;
        }
    };

    Ok(())
}

fn main() -> AnyRes {
    let cli = Cli::parse();
    match cli.command {
        SubCmd::Render(args) => render(args),
    }
}
