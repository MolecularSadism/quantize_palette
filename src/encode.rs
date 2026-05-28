//! Format-aware output encoding, including indexed PNG.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result, bail};
use image::RgbaImage;

use crate::palette::Palette;

/// Save a quantized image. With `indexed`, writes an 8-bit paletted PNG;
/// otherwise the format is chosen from the output extension (png, bmp, tga,
/// qoi, webp). Lossy extensions are refused.
pub fn save(img: &RgbaImage, path: &Path, palette: &Palette, indexed: bool) -> Result<()> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    if matches!(ext.as_str(), "jpg" | "jpeg") {
        bail!(
            "refusing to write lossy {} output — it would reintroduce off-palette colors; \
             use a lossless format (png, bmp, tga, qoi, webp)",
            ext
        );
    }

    if indexed {
        if ext != "png" {
            bail!("--indexed requires a .png output, got .{ext}");
        }
        write_indexed_png(img, path, palette)
    } else {
        img.save(path)
            .with_context(|| format!("writing {}", path.display()))
    }
}

fn write_indexed_png(img: &RgbaImage, path: &Path, palette: &Palette) -> Result<()> {
    let (w, h) = img.dimensions();
    let has_transparent = img.pixels().any(|p| p.0[3] == 0);
    let transparent_index = has_transparent.then_some(palette.len());
    let total = palette.len() + usize::from(has_transparent);
    if total > 256 {
        bail!("palette too large for an 8-bit indexed PNG ({total} entries, max 256)");
    }

    let mut plte = Vec::with_capacity(total * 3);
    for e in palette.entries() {
        plte.extend_from_slice(&e.srgb);
    }
    if transparent_index.is_some() {
        plte.extend_from_slice(&[0, 0, 0]);
    }

    let mut data = Vec::with_capacity((w * h) as usize);
    for p in img.pixels() {
        let [r, g, b, a] = p.0;
        let idx = if a == 0 {
            transparent_index.expect("transparent pixel implies a transparent index")
        } else {
            palette
                .index_of([r, g, b])
                .context("output pixel color is not in the palette")?
        };
        data.push(idx as u8);
    }

    let file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Indexed);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_palette(plte);
    if let Some(ti) = transparent_index {
        let mut trns = vec![255u8; ti + 1];
        trns[ti] = 0;
        encoder.set_trns(trns);
    }
    let mut writer = encoder.write_header().context("writing PNG header")?;
    writer
        .write_image_data(&data)
        .context("writing indexed PNG data")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    #[test]
    fn indexed_png_roundtrips() {
        let palette = Palette::from_colors(vec![[255, 0, 0], [0, 255, 0]]);
        let mut img: RgbaImage = ImageBuffer::new(2, 1);
        img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        img.put_pixel(1, 0, Rgba([0, 0, 0, 0])); // transparent

        let dir = std::env::temp_dir().join("quantize_palette_indexed_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.png");
        save(&img, &path, &palette, true).unwrap();

        let read = image::open(&path).unwrap().to_rgba8();
        assert_eq!(read.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(read.get_pixel(1, 0).0[3], 0);
    }

    #[test]
    fn refuses_lossy_output() {
        let palette = Palette::from_colors(vec![[0, 0, 0]]);
        let img: RgbaImage = ImageBuffer::new(1, 1);
        assert!(save(&img, Path::new("x.jpg"), &palette, false).is_err());
        assert!(save(&img, Path::new("x.jpeg"), &palette, false).is_err());
    }

    #[test]
    fn indexed_requires_png_extension() {
        let palette = Palette::from_colors(vec![[0, 0, 0]]);
        let img: RgbaImage = ImageBuffer::new(1, 1);
        assert!(save(&img, Path::new("x.bmp"), &palette, true).is_err());
    }

    #[test]
    fn indexed_rejects_oversized_palette() {
        // 256 opaque colors + a transparent pixel needs a 257th index.
        let colors: Vec<[u8; 3]> = (0..256).map(|i| [i as u8, 0, 0]).collect();
        let palette = Palette::from_colors(colors);
        let mut img: RgbaImage = ImageBuffer::new(2, 1);
        img.put_pixel(0, 0, Rgba([0, 0, 0, 255]));
        img.put_pixel(1, 0, Rgba([0, 0, 0, 0])); // transparent → needs index 256

        let dir = std::env::temp_dir().join("quantize_palette_oversize");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(save(&img, &dir.join("o.png"), &palette, true).is_err());
    }

    #[test]
    fn lossless_formats_roundtrip() {
        let palette = Palette::from_colors(vec![[12, 34, 56], [200, 100, 0]]);
        let mut img: RgbaImage = ImageBuffer::new(2, 1);
        img.put_pixel(0, 0, Rgba([12, 34, 56, 255]));
        img.put_pixel(1, 0, Rgba([200, 100, 0, 255]));

        let dir = std::env::temp_dir().join("quantize_palette_formats");
        std::fs::create_dir_all(&dir).unwrap();
        for ext in ["png", "bmp", "tga", "qoi", "webp"] {
            let path = dir.join(format!("out.{ext}"));
            save(&img, &path, &palette, false).unwrap();
            let read = image::open(&path)
                .unwrap_or_else(|e| panic!("reopening .{ext}: {e}"))
                .to_rgba8();
            assert_eq!(read.get_pixel(0, 0).0, [12, 34, 56, 255], "format {ext}");
            assert_eq!(read.get_pixel(1, 0).0, [200, 100, 0, 255], "format {ext}");
        }
    }
}
