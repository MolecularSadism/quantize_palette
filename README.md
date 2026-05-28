# quantize_palette

A command-line tool that snaps every pixel of an image onto a fixed color
palette. Color matching happens in [Oklab](https://bottosson.github.io/posts/oklab/)
space by default for perceptually accurate nearest-color selection, with
optional dithering and several palette/image format choices.

It was built for a game art pipeline — conforming externally produced or
AI-generated art to a fixed palette — but it works with any palette you hand it.
Two palettes are bundled into the binary (`rgbwk` and `kugelblitz36`), so it runs
with no palette file at all.

## Features

- **Perceptual matching** — nearest color is chosen in Oklab, not raw sRGB, so
  snapped colors look right to the eye. Selectable via `--metric`
  (`oklab`, `weighted`, `srgb`).
- **Dithering** — `--dither` offers Floyd–Steinberg, Atkinson, and ordered
  (Bayer 8×8). Error diffusion happens in linear light and only across opaque
  pixels, so it never bleeds over alpha edges.
- **Flexible alpha** — binarize at a configurable `--alpha-threshold`
  (default 128), or `--keep-alpha` to pass the original alpha through untouched
  and quantize only RGB.
- **Multiple input/output formats** — reads PNG, JPEG, WebP, BMP, TGA, GIF, QOI;
  writes any lossless raster format by output extension, plus indexed PNG.
- **Batch mode** — point it at a directory to process every image in parallel,
  optionally recursing with `-r`. Directory layout is mirrored into the output.

## Installation

```bash
cargo install --path .
```

Or build and run from the repo:

```bash
cargo run --release -- <input> [options]
```

## Examples

Two runnable examples generate their own source images and demo palette, then
write quantized variants to a directory (a temp dir by default, or a path you
pass) so you can open and compare them:

```bash
# Colored-square grid quantized with each metric (oklab / weighted / srgb),
# plus an indexed-PNG export.
cargo run --example colored_squares            # → temp dir
cargo run --example colored_squares ./out      # → ./out

# Smooth gradient on a coarse palette, showing each dither mode. Compare
# gradient_nearest.png (hard bands) against the dithered variants.
cargo run --example dither_comparison ./out
```

Each example prints the absolute path of every file it writes.

## As a library

The crate is also a library; the CLI is a thin wrapper over it:

```rust
use quantize_palette::palette::{Metric, Palette};
use quantize_palette::quantize::{quantize, AlphaMode, Dither};
use quantize_palette::encode;

let palette = Palette::load("palette.gpl".as_ref())?;
let img = image::open("sprite.png")?.to_rgba8();
let out = quantize(&img, &palette, Metric::Oklab, AlphaMode::Binarize(128), Dither::FloydSteinberg);
encode::save(&out, "sprite.quantized.png".as_ref(), &palette, false)?;
# Ok::<(), anyhow::Error>(())
```

## Usage

```bash
# Single file → writes sibling `sprite.quantized.png` (bundled rgbwk palette)
quantize_palette sprite.png

# Use the bundled 36-color kugelblitz palette
quantize_palette sprite.png --builtin kugelblitz36

# Pick a palette file explicitly
quantize_palette sprite.png --palette my_palette.gpl

# Dither (--dither alone = Floyd–Steinberg; or name a kernel)
quantize_palette sprite.png --dither
quantize_palette sprite.png --dither atkinson

# Keep the original alpha channel instead of binarizing it
quantize_palette sprite.png --keep-alpha

# Write a compact 8-bit indexed PNG
quantize_palette sprite.png --indexed

# Whole directory, recursively, into a new folder
quantize_palette ./art -r -o ./art_quantized

# Overwrite the inputs in place
quantize_palette ./art -r --in-place
```

| Flag | Description |
|------|-------------|
| `-o, --output <PATH>` | Output file or directory. Defaults to a sibling `<name>.quantized.png` (file) or mirrored layout (directory). |
| `-p, --palette <PATH>` | Palette file. Overrides `--builtin`. |
| `--builtin <NAME>` | Bundled palette when no file is given: `rgbwk` (default) or `kugelblitz36`. |
| `--dither [<KERNEL>]` | `none` (default), `floyd-steinberg` (the bare-flag default), `atkinson`, `ordered`. |
| `--metric <METRIC>` | Nearest-color metric: `oklab` (default), `weighted` (lightness-weighted Oklab), `srgb`. |
| `--alpha-threshold <0-255>` | Alpha cutoff for binarization. Default 128. |
| `--keep-alpha` | Leave alpha untouched; quantize RGB only. |
| `--indexed` | Write an 8-bit indexed (paletted) PNG instead of RGBA. |
| `-r, --recursive` | Recurse into subdirectories when the input is a directory. |
| `--in-place` | Overwrite input files instead of writing copies. |

`--keep-alpha` conflicts with `--alpha-threshold` and `--indexed` (indexed PNG
has no per-pixel alpha channel).

## Supported file formats

### Images

| Format | Read (input) | Write (output) | Notes |
|--------|:------------:|:--------------:|-------|
| PNG    | ✅ | ✅ | Also indexed PNG via `--indexed`. |
| QOI    | ✅ | ✅ | Fast lossless format. |
| BMP    | ✅ | ✅ | |
| TGA    | ✅ | ✅ | |
| WebP   | ✅ | ✅ | Output is lossless WebP. |
| GIF    | ✅ | ❌ | First frame only (no animation). |
| JPEG   | ✅ | ❌ | Lossy output is refused — it would reintroduce off-palette colors. |

Inputs are discovered by extension. Output format is chosen from the output
file's extension; lossy extensions (`.jpg`/`.jpeg`) are rejected. The default
output (no `-o`) is always `.quantized.png`, which is lossless and carries the
alpha channel cleanly.

### Palettes

| Format | Extension | Notes |
|--------|-----------|-------|
| GIMP   | `.gpl` | Headers and `#` comments skipped; fully transparent slot dropped. |
| JASC   | `.pal` | `JASC-PAL` / version / count header, then `R G B` rows. |
| Hex list | `.hex` | One `#RRGGBB` or `RRGGBB` per line (e.g. Lospec export). |
| Paint.NET | `.txt` | `AARRGGBB` per line; `;` comments and fully transparent entries dropped. |

Unknown extensions are parsed as GIMP `.gpl`.

## Future work

A few things were intentionally left out of this version:

- **Aseprite (`.ase` / `.aseprite`) input.** The highest-value addition for a
  pixel-art workflow, but it needs a dedicated decoder and decisions about layer
  flattening and per-frame handling. Deferred for now.
- **Animation (GIF/APNG multi-frame).** Currently only the first GIF frame is
  read. Full animation support means frame iteration, shared-vs-per-frame
  dithering, and timing metadata.
- **More dither/metric knobs.** The ordered-dither spread and the weighted-Oklab
  lightness factor are fixed constants; they could be exposed as flags if needed.

## How it works

1. Load the palette, converting each entry to linear sRGB and Oklab.
2. For each pixel, resolve alpha (binarize at the threshold, or keep it).
   Transparent pixels are written as `(0,0,0,0)`.
3. Convert the pixel to the metric's color space and pick the nearest palette
   entry by squared Euclidean distance.
4. With `--dither`, diffuse the per-channel linear-light error to neighboring
   opaque pixels (Floyd–Steinberg / Atkinson), or perturb via a Bayer matrix
   (ordered) before the nearest lookup.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this crate by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
