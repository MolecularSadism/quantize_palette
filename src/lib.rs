//! Quantize images onto a fixed color palette.
//!
//! Reads a palette (GIMP `.gpl`, JASC `.pal`, hex `.hex`, or Paint.NET `.txt`),
//! then snaps every pixel of an image to the nearest palette entry. Nearest-color
//! matching runs in Oklab space by default. Alpha is binarized at a configurable
//! threshold, or passed through untouched. Dithering (Floyd–Steinberg, Atkinson,
//! or ordered) is opt-in.
//!
//! # Example
//!
//! ```
//! use quantize_palette::palette::{Metric, Palette};
//! use quantize_palette::quantize::{quantize, AlphaMode, Dither};
//! use image::{ImageBuffer, Rgba, RgbaImage};
//!
//! let palette = Palette::from_colors(vec![[0, 0, 0], [255, 255, 255]]);
//! let mut img: RgbaImage = ImageBuffer::new(1, 1);
//! img.put_pixel(0, 0, Rgba([200, 200, 200, 255]));
//!
//! let out = quantize(&img, &palette, Metric::Oklab, AlphaMode::Binarize(128), Dither::None);
//! assert_eq!(out.get_pixel(0, 0).0, [255, 255, 255, 255]);
//! ```

pub mod color;
pub mod encode;
pub mod files;
pub mod palette;
pub mod quantize;
