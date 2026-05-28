//! Demo: snap a grid of colored squares onto a small demo palette.
//!
//! Generates a demo `.gpl` palette and a source image of distinct color
//! squares (most of them deliberately off-palette), then writes one quantized
//! variant per nearest-color metric so you can see how matching differs.
//!
//! Run with:
//!
//! ```text
//! cargo run --example colored_squares            # writes to a temp dir
//! cargo run --example colored_squares ./out_dir  # writes to ./out_dir
//! ```

use std::path::PathBuf;

use anyhow::Result;
use image::{ImageBuffer, Rgba, RgbaImage};
use quantize_palette::encode;
use quantize_palette::palette::{Metric, Palette};
use quantize_palette::quantize::{AlphaMode, Dither, quantize};

const CELL: u32 = 24; // pixels per square
const GRID: u32 = 8; // squares per side

fn main() -> Result<()> {
    let out_dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("quantize_palette_demo"));
    std::fs::create_dir_all(&out_dir)?;

    // Demo palette: 12 vivid anchors. Written to disk and loaded back so the
    // example also exercises the `.gpl` loader.
    let colors = vec![
        [26, 28, 44],
        [93, 39, 93],
        [177, 62, 83],
        [239, 125, 87],
        [255, 205, 117],
        [167, 240, 112],
        [56, 183, 100],
        [37, 113, 121],
        [41, 54, 111],
        [59, 93, 201],
        [65, 166, 246],
        [244, 244, 244],
    ];
    let gpl_path = out_dir.join("demo.gpl");
    write_gpl(&gpl_path, "Demo 12", &colors)?;
    let palette = Palette::load(&gpl_path)?;
    println!("Palette: {} colors → {}", palette.len(), gpl_path.display());

    let source = build_squares();
    let source_path = out_dir.join("squares_source.png");
    source.save(&source_path)?;
    println!("Source:  {}", source_path.display());

    for (metric, label) in [
        (Metric::Oklab, "oklab"),
        (Metric::Weighted, "weighted"),
        (Metric::Srgb, "srgb"),
    ] {
        let out = quantize(
            &source,
            &palette,
            metric,
            AlphaMode::Binarize(128),
            Dither::None,
        );
        let path = out_dir.join(format!("squares_{label}.png"));
        encode::save(&out, &path, &palette, false)?;
        println!("  {label:<8} → {}", path.display());
    }

    // Also show an indexed-PNG export (smaller on disk).
    let indexed = quantize(
        &source,
        &palette,
        Metric::Oklab,
        AlphaMode::Binarize(128),
        Dither::None,
    );
    let idx_path = out_dir.join("squares_indexed.png");
    encode::save(&indexed, &idx_path, &palette, true)?;
    println!("  indexed  → {}", idx_path.display());

    println!("\nOpen the PNGs in {} to compare.", out_dir.display());
    Ok(())
}

/// A grid where x sweeps red, y sweeps green, and blue follows the diagonal —
/// a broad spread of colors, most of which are not in the palette.
fn build_squares() -> RgbaImage {
    let size = GRID * CELL;
    let mut img: RgbaImage = ImageBuffer::new(size, size);
    for gy in 0..GRID {
        for gx in 0..GRID {
            let r = (gx * 255 / (GRID - 1)) as u8;
            let g = (gy * 255 / (GRID - 1)) as u8;
            let b = ((gx + gy) * 255 / (2 * (GRID - 1))) as u8;
            fill_cell(&mut img, gx, gy, Rgba([r, g, b, 255]));
        }
    }
    img
}

fn fill_cell(img: &mut RgbaImage, gx: u32, gy: u32, color: Rgba<u8>) {
    for y in 0..CELL {
        for x in 0..CELL {
            img.put_pixel(gx * CELL + x, gy * CELL + y, color);
        }
    }
}

fn write_gpl(path: &std::path::Path, name: &str, colors: &[[u8; 3]]) -> Result<()> {
    let mut text = format!("GIMP Palette\nName: {name}\nColumns: 0\n#\n");
    for c in colors {
        text.push_str(&format!("{} {} {}\tcolor\n", c[0], c[1], c[2]));
    }
    std::fs::write(path, text)?;
    Ok(())
}
