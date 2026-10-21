#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::todo, clippy::unreachable)]

pub fn cog_options() -> terrio::RasterWriteOptions {
    terrio::RasterWriteOptions {
        driver: Some(terrio::formats::RasterDriver::Cog),
        compression: Some("DEFLATE".into()),
        tile_size: Some(512),
        ..Default::default()
    }
}
