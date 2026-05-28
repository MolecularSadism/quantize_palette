//! Palette loading (multiple formats) and nearest-color lookup.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use clap::ValueEnum;

use crate::color::{linear_to_oklab, srgb_to_linear};

/// How nearest-color distance is measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Metric {
    /// Euclidean distance in Oklab (perceptually uniform). The default.
    Oklab,
    /// Oklab distance with the lightness axis weighted up, so matches preserve
    /// brightness more aggressively than hue/chroma.
    Weighted,
    /// Euclidean distance in linear sRGB.
    Srgb,
}

/// Extra weight applied to the Oklab lightness delta under [`Metric::Weighted`].
const LIGHTNESS_WEIGHT: f32 = 2.0;

#[derive(Clone, Copy)]
pub struct PaletteEntry {
    /// Linear sRGB, used for dithering error accumulation and the sRGB metric.
    pub linear: [f32; 3],
    /// Oklab, used for perceptual nearest-neighbor lookup.
    pub oklab: [f32; 3],
    /// Raw 8-bit sRGB written to output pixels.
    pub srgb: [u8; 3],
}

impl PaletteEntry {
    fn from_srgb(srgb: [u8; 3]) -> Self {
        let linear = [
            srgb_to_linear(srgb[0]),
            srgb_to_linear(srgb[1]),
            srgb_to_linear(srgb[2]),
        ];
        Self {
            linear,
            oklab: linear_to_oklab(linear),
            srgb,
        }
    }
}

pub struct Palette {
    entries: Vec<PaletteEntry>,
    index: HashMap<[u8; 3], usize>,
}

impl Palette {
    /// Load a palette, dispatching on file extension:
    /// `.gpl` (GIMP), `.pal` (JASC), `.hex` (one hex color per line),
    /// `.txt` (Paint.NET, `AARRGGBB`). Unknown extensions are parsed as GIMP.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading palette {}", path.display()))?;
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        let colors = match ext.as_str() {
            "pal" => parse_jasc(&text),
            "hex" => parse_hex(&text),
            "txt" => parse_paintnet(&text),
            _ => parse_gpl(&text),
        };
        Ok(Self::from_colors(colors))
    }

    pub fn from_colors(colors: Vec<[u8; 3]>) -> Self {
        let mut entries = Vec::with_capacity(colors.len());
        let mut index = HashMap::with_capacity(colors.len());
        for srgb in colors {
            index.entry(srgb).or_insert(entries.len());
            entries.push(PaletteEntry::from_srgb(srgb));
        }
        Self { entries, index }
    }

    /// Parse a GIMP `.gpl` palette from a string.
    pub fn from_gpl_str(text: &str) -> Self {
        Self::from_colors(parse_gpl(text))
    }

    /// Built-in minimal palette: pure red, green, blue, white, and black.
    pub fn rgbwk() -> Self {
        Self::from_colors(vec![
            [0, 0, 0],
            [255, 255, 255],
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
        ])
    }

    /// Built-in 36-color Kugelblitz palette.
    pub fn kugelblitz36() -> Self {
        Self::from_gpl_str(include_str!("../palettes/kugelblitz36.gpl"))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[PaletteEntry] {
        &self.entries
    }

    /// Index of an exact sRGB color in the palette, if present.
    pub fn index_of(&self, srgb: [u8; 3]) -> Option<usize> {
        self.index.get(&srgb).copied()
    }

    /// Nearest palette entry to a linear-sRGB color under the given metric.
    pub fn nearest(&self, linear: [f32; 3], metric: Metric) -> &PaletteEntry {
        let mut best = &self.entries[0];
        let mut best_d = f32::INFINITY;
        match metric {
            Metric::Srgb => {
                for e in &self.entries {
                    let d = sq_dist(linear, e.linear);
                    if d < best_d {
                        best_d = d;
                        best = e;
                    }
                }
            }
            Metric::Oklab => {
                let target = linear_to_oklab(linear);
                for e in &self.entries {
                    let d = sq_dist(target, e.oklab);
                    if d < best_d {
                        best_d = d;
                        best = e;
                    }
                }
            }
            Metric::Weighted => {
                let target = linear_to_oklab(linear);
                for e in &self.entries {
                    let dl = (target[0] - e.oklab[0]) * LIGHTNESS_WEIGHT;
                    let da = target[1] - e.oklab[1];
                    let db = target[2] - e.oklab[2];
                    let d = dl * dl + da * da + db * db;
                    if d < best_d {
                        best_d = d;
                        best = e;
                    }
                }
            }
        }
        best
    }
}

fn sq_dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d0 = a[0] - b[0];
    let d1 = a[1] - b[1];
    let d2 = a[2] - b[2];
    d0 * d0 + d1 * d1 + d2 * d2
}

// ---------------------------------------------------------------------------
// Format parsers — each returns the opaque colors, dropping fully transparent
// entries (the binarized output never selects them).
// ---------------------------------------------------------------------------

/// GIMP `.gpl`: `R G B [A]\tName` rows, with `GIMP Palette` / `Name:` /
/// `Columns:` / `Channels:` headers and `#` comments.
fn parse_gpl(text: &str) -> Vec<[u8; 3]> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with(|c: char| c.is_ascii_alphabetic()) {
            continue;
        }
        let nums: Vec<u8> = line
            .split_whitespace()
            .take_while(|t| t.chars().all(|c| c.is_ascii_digit()))
            .filter_map(|t| t.parse::<u8>().ok())
            .collect();
        if nums.len() < 3 {
            continue;
        }
        let alpha = if nums.len() >= 4 { nums[3] } else { 255 };
        if alpha == 0 {
            continue;
        }
        out.push([nums[0], nums[1], nums[2]]);
    }
    out
}

/// JASC `.pal`: `JASC-PAL` / `0100` / count header, then `R G B` rows.
fn parse_jasc(text: &str) -> Vec<[u8; 3]> {
    let mut out = Vec::new();
    for line in text.lines().skip(3) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let nums: Vec<u8> = line
            .split_whitespace()
            .filter_map(|t| t.parse::<u8>().ok())
            .collect();
        if nums.len() >= 3 {
            out.push([nums[0], nums[1], nums[2]]);
        }
    }
    out
}

/// One hex color per line (`#RRGGBB` or `RRGGBB`); `#`/`;` comments and blank
/// lines are ignored. Lospec's `.hex` export format.
fn parse_hex(text: &str) -> Vec<[u8; 3]> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        // A leading `#` here is a hex sigil, not a comment.
        let token = line.trim_start_matches('#');
        if let Some(rgb) = parse_hex6(token) {
            out.push(rgb);
        }
    }
    out
}

/// Paint.NET palette `.txt`: `;` comments, then one `AARRGGBB` hex per line.
/// Fully transparent entries (`AA == 00`) are dropped.
fn parse_paintnet(text: &str) -> Vec<[u8; 3]> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        let token = line.trim_start_matches('#');
        if token.len() == 8 {
            if let (Ok(a), Some(rgb)) = (
                u8::from_str_radix(&token[0..2], 16),
                parse_hex6(&token[2..8]),
            ) && a != 0
            {
                out.push(rgb);
            }
        } else if let Some(rgb) = parse_hex6(token) {
            out.push(rgb);
        }
    }
    out
}

fn parse_hex6(token: &str) -> Option<[u8; 3]> {
    if token.len() != 6 || !token.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&token[0..2], 16).ok()?;
    let g = u8::from_str_radix(&token[2..4], 16).ok()?;
    let b = u8::from_str_radix(&token[4..6], 16).ok()?;
    Some([r, g, b])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_picks_exact_match() {
        let palette = Palette::from_colors(vec![[0, 0, 0], [255, 255, 255], [255, 0, 0]]);
        let target = [srgb_to_linear(255), srgb_to_linear(0), srgb_to_linear(0)];
        assert_eq!(palette.nearest(target, Metric::Oklab).srgb, [255, 0, 0]);
        assert_eq!(palette.nearest(target, Metric::Srgb).srgb, [255, 0, 0]);
        assert_eq!(palette.nearest(target, Metric::Weighted).srgb, [255, 0, 0]);
    }

    #[test]
    fn builtin_rgbwk_has_five_pure_colors() {
        let palette = Palette::rgbwk();
        assert_eq!(palette.len(), 5);
        for c in [
            [0, 0, 0],
            [255, 255, 255],
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
        ] {
            assert!(palette.index_of(c).is_some(), "missing {c:?}");
        }
    }

    #[test]
    fn builtin_kugelblitz36_loads_without_transparent_slot() {
        let palette = Palette::kugelblitz36();
        assert_eq!(palette.len(), 36);
        // The transparent slot in the .gpl must be dropped.
        assert!(palette.index_of([18, 14, 18]).is_some(), "Black missing");
    }

    #[test]
    fn parses_gpl_skipping_headers_and_transparent() {
        let text = "GIMP Palette\nChannels: RGBA\n#\n  0   0   0   0\tTransparent\n255 0 0 255\tRed\n  0 255   0 255\tGreen\n";
        let colors = parse_gpl(text);
        assert_eq!(colors, vec![[255, 0, 0], [0, 255, 0]]);
    }

    #[test]
    fn parses_jasc_pal() {
        let text = "JASC-PAL\n0100\n2\n255 0 0\n0 255 0\n";
        assert_eq!(parse_jasc(text), vec![[255, 0, 0], [0, 255, 0]]);
    }

    #[test]
    fn parses_hex_list() {
        let text = "; a comment\n#ff0000\n00ff00\n";
        assert_eq!(parse_hex(text), vec![[255, 0, 0], [0, 255, 0]]);
    }

    #[test]
    fn parses_paintnet_dropping_transparent() {
        let text = ";paint.net Palette File\nFFFF0000\n00FFFFFF\nFF00FF00\n";
        assert_eq!(parse_paintnet(text), vec![[255, 0, 0], [0, 255, 0]]);
    }

    #[test]
    fn index_of_finds_color() {
        let palette = Palette::from_colors(vec![[1, 2, 3], [4, 5, 6]]);
        assert_eq!(palette.index_of([4, 5, 6]), Some(1));
        assert_eq!(palette.index_of([9, 9, 9]), None);
    }

    #[test]
    fn gpl_accepts_rgb_rows_without_alpha() {
        let text = "GIMP Palette\n10 20 30\t a\n40 50 60\n";
        assert_eq!(parse_gpl(text), vec![[10, 20, 30], [40, 50, 60]]);
    }

    #[test]
    fn duplicate_colors_index_to_first_occurrence() {
        let palette = Palette::from_colors(vec![[7, 7, 7], [9, 9, 9], [7, 7, 7]]);
        assert_eq!(palette.len(), 3);
        assert_eq!(palette.index_of([7, 7, 7]), Some(0));
    }

    #[test]
    fn weighted_metric_favors_matching_lightness() {
        // A mid gray sits between a same-lightness blue-gray and a near-black.
        // Plain Oklab may pick the closer hue; weighted should keep brightness.
        let dark = [20, 20, 20];
        let same_l = [128, 110, 150];
        let palette = Palette::from_colors(vec![dark, same_l]);
        let target = [
            srgb_to_linear(128),
            srgb_to_linear(128),
            srgb_to_linear(128),
        ];
        assert_eq!(palette.nearest(target, Metric::Weighted).srgb, same_l);
    }

    #[test]
    fn unknown_extension_loads_as_gpl() {
        let dir = std::env::temp_dir().join("quantize_palette_palette_ext");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("colors.palette");
        std::fs::write(&path, "GIMP Palette\n255 0 0\n0 255 0\n").unwrap();
        let palette = Palette::load(&path).unwrap();
        assert_eq!(palette.len(), 2);
        assert_eq!(palette.index_of([255, 0, 0]), Some(0));
    }

    #[test]
    fn load_dispatches_on_extension() {
        let dir = std::env::temp_dir().join("quantize_palette_palette_dispatch");
        std::fs::create_dir_all(&dir).unwrap();

        let hex = dir.join("p.hex");
        std::fs::write(&hex, "ff0000\n00ff00\n0000ff\n").unwrap();
        assert_eq!(Palette::load(&hex).unwrap().len(), 3);

        let pal = dir.join("p.pal");
        std::fs::write(&pal, "JASC-PAL\n0100\n1\n12 34 56\n").unwrap();
        let loaded = Palette::load(&pal).unwrap();
        assert_eq!(loaded.index_of([12, 34, 56]), Some(0));
    }
}
