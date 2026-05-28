//! Command-line front end for the `quantize_palette` library.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use rayon::prelude::*;

use quantize_palette::palette::{Metric, Palette};
use quantize_palette::quantize::{AlphaMode, Dither};
use quantize_palette::{encode, files, quantize};

/// A palette bundled into the binary, used when no `--palette` file is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Builtin {
    /// Pure red, green, blue, white, and black.
    Rgbwk,
    /// The 36-color Kugelblitz palette.
    Kugelblitz36,
}

#[derive(Parser)]
#[command(about = "Quantize image colors onto a fixed palette", long_about = None)]
struct Args {
    /// Input image file or directory of images.
    input: PathBuf,

    /// Output file or directory. Defaults to a sibling `<name>.quantized.png`
    /// for files, mirrored layout for directories.
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Palette file: GIMP `.gpl`, JASC `.pal`, hex `.hex`, or Paint.NET `.txt`.
    /// Overrides `--builtin`.
    #[arg(short, long)]
    palette: Option<PathBuf>,

    /// Bundled palette to use when no `--palette` file is given.
    #[arg(long, value_enum, default_value = "rgbwk", conflicts_with = "palette")]
    builtin: Builtin,

    /// Dithering strategy. `--dither` alone selects Floyd–Steinberg.
    #[arg(
        long,
        value_enum,
        num_args = 0..=1,
        default_value = "none",
        default_missing_value = "floyd-steinberg",
    )]
    dither: Dither,

    /// Nearest-color distance metric.
    #[arg(long, value_enum, default_value = "oklab")]
    metric: Metric,

    /// Alpha cutoff: pixels with alpha below this become fully transparent,
    /// the rest become fully opaque.
    #[arg(long, default_value_t = 128, conflicts_with = "keep_alpha")]
    alpha_threshold: u8,

    /// Leave the alpha channel untouched; quantize RGB only.
    #[arg(long)]
    keep_alpha: bool,

    /// Write an 8-bit indexed (paletted) PNG instead of RGBA.
    #[arg(long, conflicts_with = "keep_alpha")]
    indexed: bool,

    /// Recurse into subdirectories when the input is a directory.
    #[arg(short, long)]
    recursive: bool,

    /// Overwrite input files in place.
    #[arg(long)]
    in_place: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let (palette, source) = match &args.palette {
        Some(path) => (Palette::load(path)?, path.display().to_string()),
        None => match args.builtin {
            Builtin::Rgbwk => (Palette::rgbwk(), "bundled rgbwk palette".to_string()),
            Builtin::Kugelblitz36 => (
                Palette::kugelblitz36(),
                "bundled kugelblitz36 palette".to_string(),
            ),
        },
    };
    if palette.is_empty() {
        bail!("palette has no opaque colors");
    }
    eprintln!("Loaded {} colors from {source}", palette.len());

    let inputs = files::collect_inputs(&args.input, args.recursive)?;
    if inputs.is_empty() {
        bail!(
            "no supported image inputs found at {}",
            args.input.display()
        );
    }

    let alpha = if args.keep_alpha {
        AlphaMode::Keep
    } else {
        AlphaMode::Binarize(args.alpha_threshold)
    };

    let results: Vec<Result<()>> = inputs
        .par_iter()
        .map(|input| process(input, &args, &palette, alpha))
        .collect();
    for result in results {
        result?;
    }

    Ok(())
}

fn process(
    input: &std::path::Path,
    args: &Args,
    palette: &Palette,
    alpha: AlphaMode,
) -> Result<()> {
    let output = files::resolve_output(input, &args.input, args.output.as_deref(), args.in_place)?;
    let img = image::open(input)
        .with_context(|| format!("opening {}", input.display()))?
        .to_rgba8();
    let out = quantize::quantize(&img, palette, args.metric, alpha, args.dither);
    encode::save(&out, &output, palette, args.indexed)
        .with_context(|| format!("quantizing {}", input.display()))?;
    eprintln!("  {} → {}", input.display(), output.display());
    Ok(())
}
