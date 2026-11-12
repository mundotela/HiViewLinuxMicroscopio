// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Display formatting for measurement values (docs/ARCHITECTURE.md
//! §6.9): mm with two decimals, µm below one millimetre, angles with
//! one decimal, and the `± X` uncertainty of one image pixel.

/// PT-BR decimal comma ("1,25") — the UI locale.
fn pt(value: f64, decimals: usize) -> String {
    format!("{value:.decimals$}").replace('.', ",")
}

/// Formats a length in image pixels for display (§6.9):
/// - no calibration → `"123 px"` (whole pixels, no ±);
/// - ≥ 1 mm → `"1,25 mm ± 0,01"`;
/// - < 1 mm → `"523 µm ± 11"`.
///
/// `± X` is one image pixel converted by the active calibration.
#[must_use]
pub fn format_length_px(px: f64, px_per_mm: Option<f64>) -> String {
    let Some(ppm) = px_per_mm.filter(|p| p.is_finite() && *p > 0.0) else {
        return format!("{:.0} px", px.round());
    };
    let mm = px / ppm;
    let unc_mm = 1.0 / ppm;
    if mm >= 1.0 {
        format!("{} mm ± {}", pt(mm, 2), pt(unc_mm, 2))
    } else {
        format!("{} µm ± {}", pt(mm * 1000.0, 0), pt(unc_mm * 1000.0, 0))
    }
}

/// Formats an angle for display: one decimal, degrees (`"45,3°"`).
#[must_use]
pub fn format_angle_deg(deg: f64) -> String {
    format!("{}°", pt(deg, 1))
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_formatting_rules() {
        // No calibration → plain pixels.
        assert_eq!(format_length_px(123.4, None), "123 px");
        assert_eq!(format_length_px(123.4, Some(0.0)), "123 px");
        // ≥ 1 mm → two decimals, mm, ± one pixel in mm.
        assert_eq!(format_length_px(111.25, Some(89.0)), "1,25 mm ± 0,01");
        assert_eq!(format_length_px(89.0, Some(89.0)), "1,00 mm ± 0,01");
        // < 1 mm → whole µm, ± one pixel in µm.
        assert_eq!(format_length_px(44.5, Some(89.0)), "500 µm ± 11");
        assert_eq!(format_length_px(2.0, Some(89.0)), "22 µm ± 11");
    }

    #[test]
    fn angle_formatting_one_decimal() {
        assert_eq!(format_angle_deg(45.34), "45,3°");
        assert_eq!(format_angle_deg(90.0), "90,0°");
    }
}
