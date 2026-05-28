//! Pixel quantization: nearest-color and dithered mapping onto a palette.

use clap::ValueEnum;
use image::{ImageBuffer, Rgba, RgbaImage};

use crate::color::srgb_to_linear;
use crate::palette::{Metric, Palette};

/// How the alpha channel is treated.
#[derive(Clone, Copy)]
pub enum AlphaMode {
    /// Snap alpha to 0 or 255 at the given threshold (`< t` → transparent).
    Binarize(u8),
    /// Pass the original alpha through untouched; only RGB is quantized.
    Keep,
}

/// Dithering strategy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Dither {
    /// Flat nearest-neighbor, no dithering.
    None,
    /// Floyd–Steinberg error diffusion.
    FloydSteinberg,
    /// Atkinson error diffusion (lighter, common for pixel art).
    Atkinson,
    /// Ordered (Bayer 8×8) dithering — parallelizable, no error diffusion.
    Ordered,
}

/// Error-diffusion neighbor offsets and weights, normalized by `divisor`.
struct Kernel {
    taps: &'static [(i64, i64, f32)],
    divisor: f32,
}

const FLOYD_STEINBERG: Kernel = Kernel {
    taps: &[(1, 0, 7.0), (-1, 1, 3.0), (0, 1, 5.0), (1, 1, 1.0)],
    divisor: 16.0,
};

const ATKINSON: Kernel = Kernel {
    taps: &[
        (1, 0, 1.0),
        (2, 0, 1.0),
        (-1, 1, 1.0),
        (0, 1, 1.0),
        (1, 1, 1.0),
        (0, 2, 1.0),
    ],
    divisor: 8.0,
};

/// Bayer 8×8 threshold matrix.
const BAYER8: [[u8; 8]; 8] = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
];

/// Perturbation magnitude (linear light) for ordered dithering.
const ORDERED_SPREAD: f32 = 0.06;

pub fn quantize(
    img: &RgbaImage,
    palette: &Palette,
    metric: Metric,
    alpha: AlphaMode,
    dither: Dither,
) -> RgbaImage {
    match dither {
        Dither::None => map_nearest(img, palette, metric, alpha),
        Dither::Ordered => map_ordered(img, palette, metric, alpha),
        Dither::FloydSteinberg => diffuse(img, palette, metric, alpha, &FLOYD_STEINBERG),
        Dither::Atkinson => diffuse(img, palette, metric, alpha, &ATKINSON),
    }
}

/// Decide the fate of a pixel's alpha. `Some(out_alpha)` means the pixel is
/// opaque enough to quantize; `None` means write fully transparent.
fn classify(a: u8, alpha: AlphaMode) -> Option<u8> {
    match alpha {
        AlphaMode::Binarize(t) => (a >= t).then_some(255),
        AlphaMode::Keep => (a > 0).then_some(a),
    }
}

fn map_nearest(img: &RgbaImage, palette: &Palette, metric: Metric, alpha: AlphaMode) -> RgbaImage {
    let (w, h) = img.dimensions();
    let mut out = ImageBuffer::new(w, h);
    for (x, y, px) in img.enumerate_pixels() {
        let [r, g, b, a] = px.0;
        match classify(a, alpha) {
            None => out.put_pixel(x, y, Rgba([0, 0, 0, 0])),
            Some(oa) => {
                let lin = [srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b)];
                let e = palette.nearest(lin, metric);
                out.put_pixel(x, y, Rgba([e.srgb[0], e.srgb[1], e.srgb[2], oa]));
            }
        }
    }
    out
}

fn map_ordered(img: &RgbaImage, palette: &Palette, metric: Metric, alpha: AlphaMode) -> RgbaImage {
    let (w, h) = img.dimensions();
    let mut out = ImageBuffer::new(w, h);
    for (x, y, px) in img.enumerate_pixels() {
        let [r, g, b, a] = px.0;
        match classify(a, alpha) {
            None => out.put_pixel(x, y, Rgba([0, 0, 0, 0])),
            Some(oa) => {
                let t = (f32::from(BAYER8[(y % 8) as usize][(x % 8) as usize]) + 0.5) / 64.0 - 0.5;
                let bias = t * ORDERED_SPREAD;
                let lin = [
                    (srgb_to_linear(r) + bias).clamp(0.0, 1.0),
                    (srgb_to_linear(g) + bias).clamp(0.0, 1.0),
                    (srgb_to_linear(b) + bias).clamp(0.0, 1.0),
                ];
                let e = palette.nearest(lin, metric);
                out.put_pixel(x, y, Rgba([e.srgb[0], e.srgb[1], e.srgb[2], oa]));
            }
        }
    }
    out
}

fn diffuse(
    img: &RgbaImage,
    palette: &Palette,
    metric: Metric,
    alpha: AlphaMode,
    kernel: &Kernel,
) -> RgbaImage {
    let (w, h) = img.dimensions();
    let (wi, hi) = (w as usize, h as usize);

    let mut buf = vec![[0f32; 3]; wi * hi];
    let mut out_alpha = vec![None; wi * hi];
    for (i, px) in img.pixels().enumerate() {
        let [r, g, b, a] = px.0;
        out_alpha[i] = classify(a, alpha);
        buf[i] = [srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b)];
    }

    let mut out = ImageBuffer::new(w, h);
    for y in 0..hi {
        for x in 0..wi {
            let idx = y * wi + x;
            let Some(oa) = out_alpha[idx] else {
                out.put_pixel(x as u32, y as u32, Rgba([0, 0, 0, 0]));
                continue;
            };
            let target = [
                buf[idx][0].clamp(0.0, 1.0),
                buf[idx][1].clamp(0.0, 1.0),
                buf[idx][2].clamp(0.0, 1.0),
            ];
            let e = palette.nearest(target, metric);
            out.put_pixel(
                x as u32,
                y as u32,
                Rgba([e.srgb[0], e.srgb[1], e.srgb[2], oa]),
            );

            // Diffuse error in linear light, only into opaque neighbors so the
            // dither never bleeds across alpha edges.
            let err = [
                buf[idx][0] - e.linear[0],
                buf[idx][1] - e.linear[1],
                buf[idx][2] - e.linear[2],
            ];
            for &(dx, dy, weight) in kernel.taps {
                let nx = x as i64 + dx;
                let ny = y as i64 + dy;
                if nx < 0 || ny < 0 || nx >= wi as i64 || ny >= hi as i64 {
                    continue;
                }
                let n = ny as usize * wi + nx as usize;
                if out_alpha[n].is_none() {
                    continue;
                }
                let f = weight / kernel.divisor;
                buf[n][0] += err[0] * f;
                buf[n][1] += err[1] * f;
                buf[n][2] += err[2] * f;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Palette {
        Palette::from_colors(vec![[255, 0, 0], [0, 255, 0]])
    }

    #[test]
    fn binarize_drops_low_alpha() {
        let mut img: RgbaImage = ImageBuffer::new(2, 1);
        img.put_pixel(0, 0, Rgba([10, 250, 10, 127])); // below threshold
        img.put_pixel(1, 0, Rgba([10, 250, 10, 200])); // above threshold

        let out = quantize(
            &img,
            &palette(),
            Metric::Oklab,
            AlphaMode::Binarize(128),
            Dither::None,
        );
        assert_eq!(out.get_pixel(0, 0).0, [0, 0, 0, 0]);
        assert_eq!(out.get_pixel(1, 0).0, [0, 255, 0, 255]);
    }

    #[test]
    fn keep_alpha_preserves_partial_alpha() {
        let mut img: RgbaImage = ImageBuffer::new(2, 1);
        img.put_pixel(0, 0, Rgba([10, 250, 10, 0])); // fully transparent → dropped
        img.put_pixel(1, 0, Rgba([10, 250, 10, 100])); // partial alpha → kept

        let out = quantize(
            &img,
            &palette(),
            Metric::Oklab,
            AlphaMode::Keep,
            Dither::None,
        );
        assert_eq!(out.get_pixel(0, 0).0, [0, 0, 0, 0]);
        let kept = out.get_pixel(1, 0).0;
        assert_eq!([kept[0], kept[1], kept[2]], [0, 255, 0]);
        assert_eq!(kept[3], 100);
    }

    #[test]
    fn dithered_output_stays_on_palette() {
        let mut img: RgbaImage = ImageBuffer::new(4, 4);
        for p in img.pixels_mut() {
            *p = Rgba([128, 128, 0, 255]);
        }
        for kernel in [Dither::FloydSteinberg, Dither::Atkinson, Dither::Ordered] {
            let out = quantize(
                &img,
                &palette(),
                Metric::Oklab,
                AlphaMode::Binarize(128),
                kernel,
            );
            for p in out.pixels() {
                let rgb = [p.0[0], p.0[1], p.0[2]];
                assert!(
                    rgb == [255, 0, 0] || rgb == [0, 255, 0],
                    "off-palette: {rgb:?}"
                );
            }
        }
    }

    #[test]
    fn all_kernels_keep_transparent_pixels_transparent() {
        let mut img: RgbaImage = ImageBuffer::new(3, 1);
        img.put_pixel(0, 0, Rgba([200, 50, 50, 255]));
        img.put_pixel(1, 0, Rgba([50, 200, 50, 0])); // fully transparent gap
        img.put_pixel(2, 0, Rgba([50, 50, 200, 255]));
        for kernel in [
            Dither::None,
            Dither::FloydSteinberg,
            Dither::Atkinson,
            Dither::Ordered,
        ] {
            let out = quantize(
                &img,
                &palette(),
                Metric::Oklab,
                AlphaMode::Binarize(128),
                kernel,
            );
            assert_eq!(out.get_pixel(1, 0).0, [0, 0, 0, 0], "kernel {kernel:?}");
            assert_eq!(out.get_pixel(0, 0).0[3], 255);
            assert_eq!(out.get_pixel(2, 0).0[3], 255);
        }
    }

    #[test]
    fn ordered_dither_is_deterministic() {
        let mut img: RgbaImage = ImageBuffer::new(16, 16);
        for (i, p) in img.pixels_mut().enumerate() {
            let v = (i % 256) as u8;
            *p = Rgba([v, 255 - v, 128, 255]);
        }
        let a = quantize(
            &img,
            &palette(),
            Metric::Oklab,
            AlphaMode::Binarize(128),
            Dither::Ordered,
        );
        let b = quantize(
            &img,
            &palette(),
            Metric::Oklab,
            AlphaMode::Binarize(128),
            Dither::Ordered,
        );
        assert_eq!(a.into_raw(), b.into_raw());
    }

    #[test]
    fn dither_spreads_error_in_flat_region() {
        // A flat mid-tone between two palette colors should dither to a mix of
        // both, not collapse to a single color the way nearest-neighbor does.
        let mut img: RgbaImage = ImageBuffer::new(8, 8);
        for p in img.pixels_mut() {
            *p = Rgba([130, 125, 0, 255]);
        }
        let pal = palette();
        let flat = quantize(
            &img,
            &pal,
            Metric::Oklab,
            AlphaMode::Binarize(128),
            Dither::None,
        );
        let dithered = quantize(
            &img,
            &pal,
            Metric::Oklab,
            AlphaMode::Binarize(128),
            Dither::FloydSteinberg,
        );

        let distinct = |img: &RgbaImage| {
            img.pixels()
                .map(|p| [p.0[0], p.0[1], p.0[2]])
                .collect::<std::collections::HashSet<_>>()
                .len()
        };
        assert_eq!(distinct(&flat), 1, "nearest should be uniform");
        assert!(distinct(&dithered) > 1, "dither should mix colors");
    }
}
