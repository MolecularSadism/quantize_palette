//! sRGB / linear / Oklab color-space conversions.

/// 8-bit sRGB channel → linear sRGB.
pub fn srgb_to_linear(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear sRGB → Oklab. Coefficients from Björn Ottosson's reference impl
/// (<https://bottosson.github.io/posts/oklab/>).
pub fn linear_to_oklab(rgb: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = rgb;
    let l = 0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
    let l_ = l.cbrt();
    let m_ = m.cbrt();
    let s_ = s.cbrt();
    [
        0.210_454_26 * l_ + 0.793_617_8 * m_ - 0.004_072_047 * s_,
        1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_,
        0.025_904_037 * l_ + 0.782_771_77 * m_ - 0.808_675_77 * s_,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_endpoints_map_to_unit_range() {
        assert_eq!(srgb_to_linear(0), 0.0);
        assert!((srgb_to_linear(255) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn srgb_to_linear_is_monotonic() {
        let mut prev = -1.0;
        for c in 0..=255u8 {
            let v = srgb_to_linear(c);
            assert!(v > prev, "not increasing at {c}");
            prev = v;
        }
    }

    #[test]
    fn oklab_of_black_is_origin() {
        let lab = linear_to_oklab([0.0, 0.0, 0.0]);
        assert!(lab.iter().all(|c| c.abs() < 1e-6), "{lab:?}");
    }

    #[test]
    fn oklab_of_white_is_neutral_lightness() {
        let [l, a, b] = linear_to_oklab([1.0, 1.0, 1.0]);
        assert!((l - 1.0).abs() < 1e-3, "L = {l}");
        assert!(a.abs() < 1e-3 && b.abs() < 1e-3, "a = {a}, b = {b}");
    }

    #[test]
    fn oklab_red_is_warm() {
        // Red should have positive a (toward red) and positive b (toward yellow).
        let [_, a, b] = linear_to_oklab([1.0, 0.0, 0.0]);
        assert!(a > 0.0, "a = {a}");
        assert!(b > 0.0, "b = {b}");
    }
}
