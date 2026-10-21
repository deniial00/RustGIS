use rustgis_data::raster::{ColorHint, Crs, GeoTransform, Raster};

use crate::Error;

pub const CASES: &[&str] = &["rgb", "float-nodata", "uint16", "palette", "classes", "gray-alpha", "rgba", "odd", "bigtiff"];

/// Construct the same planar f32 RustGIS input for either writer; no input decoding in timings.
pub fn make(case: &str, width: u32, height: u32) -> Result<Raster, Error> {
    let n = (width as usize).checked_mul(height as usize).ok_or("pixel count overflow")?;
    let (count, color, data_type, nodata) = match case {
        "rgb" | "odd" | "bigtiff" => (3, ColorHint::Rgb { alpha: false }, "UInt8", None),
        "float-nodata" => (1, ColorHint::Gray, "Float32", Some(-9999.0)),
        "uint16" => (1, ColorHint::Gray, "UInt16", Some(65535.0)),
        "classes" => (1, ColorHint::Gray, "UInt8", None),
        "palette" => {
            let mut colors = vec![[0, 0, 0, 255]; 256];
            for (i, c) in [(1, [220, 30, 40, 255]), (7, [20, 210, 50, 255]), (19, [20, 40, 230, 255])] {
                if let Some(entry) = colors.get_mut(i) {
                    *entry = c;
                }
            }
            (1, ColorHint::Palette(colors), "UInt8", None)
        }
        "gray-alpha" => (2, ColorHint::GrayAlpha, "UInt8", None),
        "rgba" => (4, ColorHint::Rgb { alpha: true }, "UInt8", None),
        _ => return Err(format!("unknown case {case}; choose {}", CASES.join(", ")).into()),
    };
    // A malformed CLI/JS request must not cause an unbounded allocation.
    let max_samples = rustgis_data::raster::MAX_SAMPLES.min(64 * 1024 * 1024);
    if n == 0 || n.checked_mul(count).is_none_or(|samples| samples > max_samples) {
        return Err(format!("fixture must contain 1..={max_samples} samples total").into());
    }
    let mut bands = Vec::with_capacity(count);
    for b in 0..count {
        let mut values = Vec::new();
        values.try_reserve_exact(n)?;
        for y in 0..height {
            for x in 0..width {
                let hole = x % 17 == 0 && y % 19 == 0;
                let value = match case {
                    "float-nodata" => {
                        if hole {
                            -9999.0
                        } else {
                            ((u64::from(x) * 3 + u64::from(y) * 5) % 1000) as f32 / 4.0
                        }
                    }
                    "uint16" => {
                        if hole {
                            65535.0
                        } else {
                            ((u64::from(x) * 137 + u64::from(y) * 251) % 65535) as f32
                        }
                    }
                    "classes" | "palette" => match (u64::from(x) + u64::from(y)) % 3 {
                        0 => 1.0,
                        1 => 7.0,
                        _ => 19.0,
                    },
                    "gray-alpha" => {
                        if x % 2 != 0 {
                            0.0
                        } else if b == 0 {
                            200.0
                        } else {
                            255.0
                        }
                    }
                    "rgba" => {
                        if x % 2 != 0 {
                            0.0
                        } else {
                            match b {
                                0 => 200.0,
                                1 => 100.0,
                                2 => 50.0,
                                _ => 255.0,
                            }
                        }
                    }
                    _ => match b {
                        0 => (x % 256) as f32,
                        1 => (y % 256) as f32,
                        _ => {
                            if (x / 2 + y / 2) % 2 == 0 {
                                200.0
                            } else {
                                100.0
                            }
                        }
                    },
                };
                values.push(value);
            }
        }
        bands.push(values);
    }
    let transform = GeoTransform { origin_x: 500000.0, origin_y: 4000000.0, pixel_width: 30.0, pixel_height: -30.0 };
    Ok(Raster::new(
        case,
        width,
        height,
        bands,
        transform,
        Crs::from_epsg(32617).ok_or("unsupported fixture EPSG")?,
        nodata,
        color,
        data_type,
    )?)
}
