//! Input discovery and output path resolution.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use walkdir::WalkDir;

/// Image extensions the tool will read.
const INPUT_EXTENSIONS: [&str; 8] = ["png", "jpg", "jpeg", "webp", "bmp", "tga", "gif", "qoi"];

pub fn has_supported_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let e = e.to_ascii_lowercase();
            INPUT_EXTENSIONS.contains(&e.as_str())
        })
        .unwrap_or(false)
}

pub fn collect_inputs(input: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    if input.is_file() {
        return Ok(vec![input.to_path_buf()]);
    }
    if !input.is_dir() {
        bail!("input path does not exist: {}", input.display());
    }
    let max_depth = if recursive { usize::MAX } else { 1 };
    let mut out = Vec::new();
    for entry in WalkDir::new(input).max_depth(max_depth) {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && has_supported_extension(path) {
            out.push(path.to_path_buf());
        }
    }
    out.sort();
    Ok(out)
}

/// Resolve where a given input writes to, honoring `--in-place` / `--output`.
pub fn resolve_output(
    input: &Path,
    input_root: &Path,
    output: Option<&Path>,
    in_place: bool,
) -> Result<PathBuf> {
    if in_place {
        return Ok(input.to_path_buf());
    }
    let Some(output) = output else {
        // Sibling file with a `.quantized.png` suffix (lossless by default).
        let stem = input
            .file_stem()
            .and_then(|s| s.to_str())
            .with_context(|| format!("invalid filename: {}", input.display()))?;
        let parent = input.parent().unwrap_or_else(|| Path::new(""));
        return Ok(parent.join(format!("{stem}.quantized.png")));
    };

    // Directory input → mirror the layout under the output directory.
    if input_root.is_dir() {
        let rel = input.strip_prefix(input_root).unwrap_or(input);
        let dest = output.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        return Ok(dest);
    }

    // Single file input: treat output as a directory if it is one, else a file.
    if output.exists() && output.is_dir() {
        let file = input.file_name().context("input has no filename")?;
        return Ok(output.join(file));
    }
    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    Ok(output.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_matching_is_case_insensitive() {
        assert!(has_supported_extension(Path::new("a.png")));
        assert!(has_supported_extension(Path::new("a.PNG")));
        assert!(has_supported_extension(Path::new("a.JpG")));
        assert!(has_supported_extension(Path::new("a.qoi")));
        assert!(!has_supported_extension(Path::new("a.txt")));
        assert!(!has_supported_extension(Path::new("a")));
    }

    #[test]
    fn collect_inputs_respects_recursion() {
        let root = std::env::temp_dir().join("quantize_palette_collect");
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(root.join("a.png"), b"x").unwrap();
        std::fs::write(root.join("note.txt"), b"x").unwrap();
        std::fs::write(sub.join("b.png"), b"x").unwrap();

        let shallow = collect_inputs(&root, false).unwrap();
        assert_eq!(shallow.len(), 1, "shallow should skip subdirs: {shallow:?}");

        let deep = collect_inputs(&root, true).unwrap();
        assert_eq!(deep.len(), 2, "recursive should include subdir: {deep:?}");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn collect_inputs_single_file_passthrough() {
        let file = std::env::temp_dir().join("quantize_palette_single.png");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(collect_inputs(&file, false).unwrap(), vec![file]);
    }

    #[test]
    fn default_output_is_sibling_quantized_png() {
        let out =
            resolve_output(Path::new("/art/sprite.jpg"), Path::new("/art"), None, false).unwrap();
        assert_eq!(out.file_name().unwrap(), "sprite.quantized.png");
    }

    #[test]
    fn in_place_returns_input() {
        let input = Path::new("/art/sprite.png");
        let out =
            resolve_output(input, Path::new("/art"), Some(Path::new("/ignored")), true).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn directory_input_mirrors_layout() {
        let root = std::env::temp_dir().join("quantize_palette_mirror_in");
        let out_root = std::env::temp_dir().join("quantize_palette_mirror_out");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let input = root.join("sub").join("img.png");
        let out = resolve_output(&input, &root, Some(&out_root), false).unwrap();
        assert_eq!(out, out_root.join("sub").join("img.png"));
        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&out_root).ok();
    }
}
