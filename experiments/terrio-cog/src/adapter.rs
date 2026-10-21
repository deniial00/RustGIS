//! Narrow, lossless RustGIS-to-Terrio conversion for the isolated COG evaluation.
//!
//! The requested overview algorithm is passed through unchanged. In the evaluated upstream
//! revision COG ignores `overview_resampling`; this adapter deliberately does not repair it.

use rustgis_data::raster::{ColorHint, MAX_SAMPLES, Raster};
use terrio::formats::RasterDriver;
use terrio::raster::RasterData;
use terrio::{DataType, RasterDataset, RasterWriteOptions};

fn invalid(message: impl Into<String>) -> crate::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into()).into()
}

/// Encode the selected scalar band, or the raster's explicitly declared colour bands.
///
/// Only explicit UInt8, UInt16 and Float32 storage is supported. Integer samples and nodata
/// must fit exactly; metadata never licenses rounding, clamping or a float fallback. All source
/// shapes are checked because `Raster` exposes mutable public dimensions and band buffers.
/// On native targets Terrio's public memory writer itself uses temporary files.
pub fn export(r: &Raster, band: usize, big: bool) -> Result<Vec<u8>, crate::Error> {
    let (dataset, options) = prepare(r, band, big)?;
    Ok(terrio::memory::write_raster_bytes(&dataset, "evaluation.tif", &options)?)
}

/// Write a new native COG without routing the result through a temporary file and byte buffer.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_file(r: &Raster, band: usize, big: bool, path: &std::path::Path) -> Result<(), crate::Error> {
    let (dataset, options) = prepare(r, band, big)?;
    let path_text = path.to_str().ok_or_else(|| invalid("Terrio requires a UTF-8 output path"))?;
    // Reserve a new target before upstream's truncating create. Never overwrite user output.
    let reservation = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    drop(reservation);
    Ok(terrio::write_raster_dataset(&dataset, path_text, &options)?)
}

fn prepare(r: &Raster, band: usize, big: bool) -> Result<(RasterDataset, RasterWriteOptions), crate::Error> {
    let cols = r.width as usize;
    let rows = r.height as usize;
    let pixels = cols.checked_mul(rows).ok_or_else(|| invalid("raster dimensions overflow"))?;
    let source_samples = pixels.checked_mul(r.bands.len()).ok_or_else(|| invalid("raster sample count overflows"))?;
    if pixels == 0 || r.bands.is_empty() || source_samples > MAX_SAMPLES {
        return Err(invalid("raster is empty or exceeds the RustGIS sample limit"));
    }
    if r.bands.iter().any(|b| b.data.len() != pixels) {
        return Err(invalid("raster band length does not match its dimensions"));
    }
    if r.bands.get(band).is_none() {
        return Err(invalid(format!("raster has no zero-based band {band}")));
    }
    let gt = r.transform;
    if !r.georeferenced || !gt.is_valid() || gt.pixel_width <= 0.0 || gt.pixel_height >= 0.0 {
        return Err(invalid("Terrio evaluation requires a finite, georeferenced north-up transform"));
    }
    let east = gt.origin_x + f64::from(r.width) * gt.pixel_width;
    let south = gt.origin_y + f64::from(r.height) * gt.pixel_height;
    if !east.is_finite() || !south.is_finite() || east <= gt.origin_x || south >= gt.origin_y {
        return Err(invalid("raster transform has nonfinite or numerically empty bounds"));
    }
    let epsg = r.crs.epsg();
    if epsg == 0 || rustgis_data::raster::Crs::from_epsg(u32::from(epsg)).is_none() {
        return Err(invalid("Terrio evaluation supports recognized EPSG coordinate systems only"));
    }
    let crs = terrio::crs::SpatialRef::from_epsg(u32::from(epsg))?.to_crs_info();
    let data_type = match r.data_type.as_str() {
        "UInt8" => DataType::U8,
        "UInt16" => DataType::U16,
        "Float32" => DataType::F32,
        other => return Err(invalid(format!("unsupported explicit raster type {other:?}"))),
    };
    validate_nodata(r.nodata, data_type)?;

    let scalar = [band];
    let (selected, photometric, alpha, palette): (&[usize], &str, bool, &[[u8; 4]]) = match &r.color {
        ColorHint::Gray => (&scalar, "MINISBLACK", false, &[]),
        ColorHint::GrayAlpha => (&[0, 1], "MINISBLACK", true, &[]),
        ColorHint::Rgb { alpha: false } => (&[0, 1, 2], "RGB", false, &[]),
        ColorHint::Rgb { alpha: true } => (&[0, 1, 2, 3], "RGB", true, &[]),
        ColorHint::Palette(colors) => {
            if band != 0 || !matches!(data_type, DataType::U8 | DataType::U16) {
                return Err(invalid("a palette requires band zero with explicit UInt8 or UInt16 indexes"));
            }
            let limit = if data_type == DataType::U8 { 256 } else { 65_536 };
            if colors.is_empty() || colors.len() > limit || colors.iter().any(|c| c[3] != 255) {
                return Err(invalid("TIFF palette must be nonempty, fit its index type and have opaque entries"));
            }
            (&scalar, "PALETTE", false, colors)
        }
    };
    if selected.iter().any(|&b| r.bands.get(b).is_none()) {
        return Err(invalid("declared colour interpretation is missing required bands"));
    }
    let samples = pixels.checked_mul(selected.len()).ok_or_else(|| invalid("output sample count overflows"))?;
    let data = match data_type {
        DataType::U8 => RasterData::U8(convert(r, selected, samples, |v| {
            if v.is_finite() && (0.0..=255.0).contains(&v) && v.fract() == 0.0 {
                Ok(v as u8)
            } else {
                Err(invalid("UInt8 sample cannot be represented without loss"))
            }
        })?),
        DataType::U16 => RasterData::U16(convert(r, selected, samples, |v| {
            if v.is_finite() && (0.0..=65_535.0).contains(&v) && v.fract() == 0.0 {
                Ok(v as u16)
            } else {
                Err(invalid("UInt16 sample cannot be represented without loss"))
            }
        })?),
        DataType::F32 => RasterData::F32(convert(r, selected, samples, |v| {
            if v.is_infinite() { Err(invalid("infinite samples are not supported by the Terrio evaluation")) } else { Ok(v) }
        })?),
        _ => return Err(invalid("unsupported output sample type")),
    };
    // Do not call from_data_native: wbraster 0.2.1 first allocates and fills a second full
    // nodata buffer in Raster::new, then drops it when moving in the provided typed buffer.
    // Public-field coupling is intentional and confined to this pinned evaluation.
    // Construct the validated representation directly without that discarded allocation.
    let raster = terrio::Raster {
        cols,
        rows,
        bands: selected.len(),
        x_min: gt.origin_x,
        y_min: south,
        cell_size_x: gt.pixel_width,
        cell_size_y: -gt.pixel_height,
        nodata: r.nodata.unwrap_or(f64::NAN),
        data_type,
        crs,
        metadata: Vec::new(),
        data,
    };
    let dataset = RasterDataset::with_driver(raster, RasterDriver::Cog, r.nodata.is_some());
    let categorical = !palette.is_empty()
        || (!alpha
            && selected.len() == 1
            && data_type != DataType::F32
            && r.bands.get(band).is_some_and(|b| b.stats.categories.as_ref().is_some_and(|c| c.len() <= 64)));
    let mut creation_options =
        vec![("PHOTOMETRIC".into(), photometric.into()), ("PREDICTOR".into(), "1".into()), ("INTERLEAVE".into(), "PIXEL".into())];
    if alpha {
        creation_options.push(("ALPHA".into(), "YES".into()));
    }
    let options = RasterWriteOptions {
        driver: Some(RasterDriver::Cog),
        compression: Some("DEFLATE".into()),
        tile_size: Some(512),
        bigtiff: big,
        creation_options,
        color_table: palette.to_vec(),
        overview_resampling: Some(if categorical { "nearest" } else { "average" }.into()),
        // Carry the original north edge explicitly; deriving it back from y_min can round.
        geo_transform: Some([gt.origin_x, gt.pixel_width, 0.0, gt.origin_y, 0.0, gt.pixel_height]),
        ..Default::default()
    };
    Ok((dataset, options))
}

fn validate_nodata(nodata: Option<f64>, data_type: DataType) -> Result<(), crate::Error> {
    let Some(v) = nodata else { return Ok(()) };
    let exact = match data_type {
        DataType::U8 => v.is_finite() && (0.0..=255.0).contains(&v) && v.fract() == 0.0,
        DataType::U16 => v.is_finite() && (0.0..=65_535.0).contains(&v) && v.fract() == 0.0,
        DataType::F32 => v.is_nan() || (v.is_finite() && f64::from(v as f32) == v),
        _ => false,
    };
    if exact { Ok(()) } else { Err(invalid("nodata cannot be represented exactly by the explicit sample type")) }
}

fn convert<T>(
    r: &Raster,
    selected: &[usize],
    samples: usize,
    mut sample: impl FnMut(f32) -> Result<T, crate::Error>,
) -> Result<Vec<T>, crate::Error> {
    let mut out = Vec::new();
    out.try_reserve_exact(samples)?;
    for &index in selected {
        let band = r.bands.get(index).ok_or_else(|| invalid("missing selected raster band"))?;
        for &value in &band.data {
            out.push(sample(value)?);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::export;
    use rustgis_data::raster::{ColorHint, Crs, GeoTransform, Raster};

    fn raster(data_type: &str) -> Raster {
        Raster::new(
            "adapter hostile input",
            2,
            1,
            vec![vec![1.0, 7.0]],
            GeoTransform { origin_x: 500_000.0, origin_y: 4_000_000.0, pixel_width: 30.0, pixel_height: -30.0 },
            Crs::Epsg(32617),
            None,
            ColorHint::Gray,
            data_type,
        )
        .expect("valid small raster")
    }

    #[test]
    fn malformed_shapes_and_band_requests_are_rejected() {
        let mut r = raster("UInt8");
        assert!(export(&r, usize::MAX, false).is_err());
        r.color = ColorHint::Rgb { alpha: false };
        assert!(export(&r, 0, false).is_err());
        r.color = ColorHint::Gray;
        r.bands[0].data.pop();
        assert!(export(&r, 0, false).is_err());
        r.width = 0;
        assert!(export(&r, 0, false).is_err());
        r.width = u32::MAX;
        r.height = u32::MAX;
        assert!(export(&r, 0, false).is_err());
    }

    #[test]
    fn explicit_integer_types_never_round_clamp_or_replace_nan() {
        for (data_type, values) in
            [("UInt8", vec![-1.0, 0.5, 256.0, f32::NAN, f32::INFINITY]), ("UInt16", vec![-1.0, 0.5, 65_536.0, f32::NAN, f32::INFINITY])]
        {
            for value in values {
                let mut r = raster(data_type);
                r.bands[0].data[0] = value;
                assert!(export(&r, 0, false).is_err(), "{data_type}: {value}");
            }
        }
    }

    #[test]
    fn lossy_nodata_and_unsupported_storage_are_rejected() {
        let mut r = raster("UInt8");
        r.nodata = Some(256.0);
        assert!(export(&r, 0, false).is_err());
        let mut r = raster("Float32");
        r.nodata = Some(0.1);
        assert!(export(&r, 0, false).is_err());
        assert!(export(&raster("UInt32"), 0, false).is_err());
        let mut r = raster("UInt8");
        r.color = ColorHint::Palette(vec![[255, 0, 0, 128]]);
        assert!(export(&r, 0, false).is_err());
    }
}
