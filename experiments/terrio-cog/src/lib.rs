#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::todo, clippy::unreachable))]

pub type Error = Box<dyn std::error::Error>;

#[cfg(feature = "terrio")]
pub mod adapter;

pub mod fixtures;

/// In-memory export for browser downloads. No filesystem in the wasm path.
pub fn encode(r: &rustgis_data::raster::Raster, writer: &str, big: bool) -> Result<Vec<u8>, Error> {
    match writer {
        "native" => {
            use rustgis_data::raster::cogwrite::{self, Flavour};
            let mut out = std::io::Cursor::new(Vec::new());
            cogwrite::write_to(r, 0, if big { Flavour::Big } else { Flavour::Auto }, &mut out)?;
            Ok(out.into_inner())
        }
        #[cfg(feature = "terrio")]
        "terrio" => adapter::export(r, 0, big),
        _ => Err(format!("writer {writer} unavailable; Terrio requires --features terrio").into()),
    }
}

/// Direct-to-file desktop path; fixture construction is outside this function.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_file(r: &rustgis_data::raster::Raster, writer: &str, big: bool, path: &std::path::Path) -> Result<u64, Error> {
    if path.exists() {
        return Err("output already exists; use a new path".into());
    }
    match writer {
        "native" => {
            use rustgis_data::raster::cogwrite::{self, Flavour};
            let file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
            let mut out = std::io::BufWriter::with_capacity(1 << 20, file);
            cogwrite::write_to(r, 0, if big { Flavour::Big } else { Flavour::Auto }, &mut out)?;
            out.into_inner()?.sync_all()?;
        }
        #[cfg(feature = "terrio")]
        "terrio" => {
            adapter::write_file(r, 0, big, path)?;
            std::fs::OpenOptions::new().write(true).open(path)?.sync_all()?;
        }
        _ => return Err(format!("writer {writer} unavailable; Terrio requires --features terrio").into()),
    }
    Ok(std::fs::metadata(path)?.len())
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn export_case(writer: &str, case: &str, width: u32, height: u32, big: bool) -> Result<Vec<u8>, wasm_bindgen::JsValue> {
    let result = fixtures::make(case, width, height).and_then(|r| encode(&r, writer, big || case == "bigtiff"));
    result.map_err(|e| wasm_bindgen::JsValue::from_str(&e.to_string()))
}
