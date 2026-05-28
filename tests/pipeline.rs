//! End-to-end tests exercising the full load → quantize → encode pipeline
//! through the public library API.

use std::collections::HashSet;
use std::path::PathBuf;

use image::{ImageBuffer, Rgba, RgbaImage};
use quantize_palette::encode;
use quantize_palette::palette::{Metric, Palette};
use quantize_palette::quantize::{AlphaMode, Dither, quantize};

/// A small vivid demo palette.
fn demo_colors() -> Vec<[u8; 3]> {
    vec![
        [0, 0, 0],
        [255, 255, 255],
        [128, 128, 128],
        [228, 59, 68],
        [99, 199, 77],
        [0, 87, 132],
        [255, 205, 117],
        [38, 43, 68],
    ]
}

fn unique_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("quantize_palette_it_{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Generate a grid of distinct (mostly off-palette) colored squares.
fn colored_squares() -> RgbaImage {
    let cells = 6;
    let cell = 4;
    let size = cells * cell;
    let mut img: RgbaImage = ImageBuffer::new(size, size);
    for cy in 0..cells {
        for cx in 0..cells {
            let r = (cx * 255 / (cells - 1)) as u8;
            let g = (cy * 255 / (cells - 1)) as u8;
            let b = ((cx + cy) * 255 / (2 * (cells - 1))) as u8;
            for y in 0..cell {
                for x in 0..cell {
                    img.put_pixel(cx * cell + x, cy * cell + y, Rgba([r, g, b, 255]));
                }
            }
        }
    }
    img
}

fn palette_colors(img: &RgbaImage) -> HashSet<[u8; 3]> {
    img.pixels().map(|p| [p.0[0], p.0[1], p.0[2]]).collect()
}

#[test]
fn output_uses_only_palette_colors_for_every_mode() {
    let palette = Palette::from_colors(demo_colors());
    let allowed: HashSet<[u8; 3]> = demo_colors().into_iter().collect();
    let img = colored_squares();

    for metric in [Metric::Oklab, Metric::Weighted, Metric::Srgb] {
        for dither in [
            Dither::None,
            Dither::FloydSteinberg,
            Dither::Atkinson,
            Dither::Ordered,
        ] {
            let out = quantize(&img, &palette, metric, AlphaMode::Binarize(128), dither);
            for color in palette_colors(&out) {
                assert!(
                    allowed.contains(&color),
                    "off-palette color {color:?} for {metric:?}/{dither:?}"
                );
            }
        }
    }
}

#[test]
fn gpl_roundtrip_through_disk() {
    let dir = unique_dir("gpl");
    let path = dir.join("demo.gpl");

    let mut text = String::from("GIMP Palette\nName: Demo\nColumns: 0\n#\n");
    for c in demo_colors() {
        text.push_str(&format!("{} {} {}\tcolor\n", c[0], c[1], c[2]));
    }
    std::fs::write(&path, text).unwrap();

    let palette = Palette::load(&path).unwrap();
    assert_eq!(palette.len(), demo_colors().len());
    for c in demo_colors() {
        assert!(palette.index_of(c).is_some(), "missing {c:?}");
    }
}

#[test]
fn keep_alpha_survives_lossless_formats() {
    let palette = Palette::from_colors(demo_colors());
    let mut img: RgbaImage = ImageBuffer::new(3, 1);
    img.put_pixel(0, 0, Rgba([230, 60, 70, 255])); // opaque
    img.put_pixel(1, 0, Rgba([100, 200, 80, 90])); // partial alpha → kept
    img.put_pixel(2, 0, Rgba([10, 90, 130, 0])); // fully transparent

    let out = quantize(&img, &palette, Metric::Oklab, AlphaMode::Keep, Dither::None);
    let dir = unique_dir("keepalpha");

    for ext in ["png", "qoi", "webp"] {
        let path = dir.join(format!("out.{ext}"));
        encode::save(&out, &path, &palette, false).unwrap();
        let read = image::open(&path).unwrap().to_rgba8();
        assert_eq!(read.get_pixel(0, 0).0[3], 255, "format {ext}");
        assert_eq!(read.get_pixel(1, 0).0[3], 90, "format {ext}");
        assert_eq!(read.get_pixel(2, 0).0[3], 0, "format {ext}");
    }
}

#[test]
fn indexed_png_matches_rgba_png() {
    let palette = Palette::from_colors(demo_colors());
    let img = colored_squares();
    let out = quantize(
        &img,
        &palette,
        Metric::Oklab,
        AlphaMode::Binarize(128),
        Dither::None,
    );

    let dir = unique_dir("indexed_match");
    let rgba_path = dir.join("rgba.png");
    let idx_path = dir.join("indexed.png");
    encode::save(&out, &rgba_path, &palette, false).unwrap();
    encode::save(&out, &idx_path, &palette, true).unwrap();

    let rgba = image::open(&rgba_path).unwrap().to_rgba8();
    let indexed = image::open(&idx_path).unwrap().to_rgba8();
    assert_eq!(rgba.dimensions(), indexed.dimensions());
    for (a, b) in rgba.pixels().zip(indexed.pixels()) {
        assert_eq!(a.0, b.0);
    }
}

#[test]
fn binarized_alpha_is_only_zero_or_full() {
    let palette = Palette::from_colors(demo_colors());
    let mut img: RgbaImage = ImageBuffer::new(4, 1);
    img.put_pixel(0, 0, Rgba([200, 50, 50, 10]));
    img.put_pixel(1, 0, Rgba([200, 50, 50, 127]));
    img.put_pixel(2, 0, Rgba([200, 50, 50, 128]));
    img.put_pixel(3, 0, Rgba([200, 50, 50, 255]));

    let out = quantize(
        &img,
        &palette,
        Metric::Oklab,
        AlphaMode::Binarize(128),
        Dither::None,
    );
    let alphas: Vec<u8> = out.pixels().map(|p| p.0[3]).collect();
    assert_eq!(alphas, vec![0, 0, 255, 255]);
}
