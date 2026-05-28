//! Demo: show how dithering recovers smooth gradients on a tiny palette.
//!
//! Generates a smooth gradient source (grayscale ramp + a hue sweep) and a
//! small demo palette, then writes one output per dither mode. Flat
//! nearest-neighbor produces hard bands; the dithered variants break them up.
//!
//! Run with:
//!
//! ```text
//! cargo run --example dither_comparison            # writes to a temp dir
//! cargo run --example dither_comparison ./out_dir  # writes to ./out_dir
//! ```

use std::path::PathBuf;

use anyhow::Result;
use image::{ImageBuffer, Rgba, RgbaImage};
use quantize_palette::encode;
use quantize_palette::palette::{Metric, Palette};
use quantize_palette::quantize::{AlphaMode, Dither, quantize};

const WIDTH: u32 = 256;
const HEIGHT: u32 = 128;

fn main() -> Result<()> {
    let out_dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("quantize_palette_demo"));
    std::fs::create_dir_all(&out_dir)?;

    // A deliberately coarse palette so banding is obvious: a 4-step gray ramp
    // plus the RGB primaries and their complements.
    let colors = vec![
        [0, 0, 0],
        [85, 85, 85],
        [170, 170, 170],
        [255, 255, 255],
        [255, 0, 0],
        [0, 255, 0],
        [0, 0, 255],
        [0, 255, 255],
        [255, 0, 255],
        [255, 255, 0],
    ];
    let palette = Palette::from_colors(colors);
    println!(
        "Palette: {} colors (coarse, to expose banding)",
        palette.len()
    );

    let source = build_gradient();
    let source_path = out_dir.join("gradient_source.png");
    source.save(&source_path)?;
    println!("Source:  {}", source_path.display());

    for (dither, label) in [
        (Dither::None, "nearest"),
        (Dither::FloydSteinberg, "floyd_steinberg"),
        (Dither::Atkinson, "atkinson"),
        (Dither::Ordered, "ordered"),
    ] {
        let out = quantize(
            &source,
            &palette,
            Metric::Oklab,
            AlphaMode::Binarize(128),
            dither,
        );
        let path = out_dir.join(format!("gradient_{label}.png"));
        encode::save(&out, &path, &palette, false)?;
        println!("  {label:<16} → {}", path.display());
    }

    println!("\nCompare gradient_nearest.png (banded) against the dithered variants.");
    Ok(())
}

/// Top half: horizontal grayscale ramp. Bottom half: horizontal hue sweep at
/// full brightness. Both are smooth, so a coarse palette bands hard.
fn build_gradient() -> RgbaImage {
    let mut img: RgbaImage = ImageBuffer::new(WIDTH, HEIGHT);
    let half = HEIGHT / 2;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let t = x as f32 / (WIDTH - 1) as f32;
            let color = if y < half {
                let v = (t * 255.0).round() as u8;
                [v, v, v]
            } else {
                hsv_to_rgb(t * 360.0, 1.0, 1.0)
            };
            img.put_pixel(x, y, Rgba([color[0], color[1], color[2], 255]));
        }
    }
    img
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 3] {
    let c = v * s;
    let h = (h % 360.0) / 60.0;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    ]
}
