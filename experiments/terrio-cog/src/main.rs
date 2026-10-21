#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::todo, clippy::unreachable)]

use rustgis_terrio_cog_eval::{Error, fixtures, write_file};

fn run() -> Result<(), Error> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [cmd, writer, case, width, height, path, rest @ ..] = args.as_slice() else {
        return Err("usage: rustgis-terrio-cog-eval export <native|terrio> <case> <width> <height> <new-output.tif> [big]".into());
    };
    if cmd != "export" || rest.len() > 1 || rest.first().is_some_and(|s| s != "big") {
        return Err("expected export command and optional big flag".into());
    }
    let width = width.parse()?;
    let height = height.parse()?;
    let r = fixtures::make(case, width, height)?;
    let start = web_time::Instant::now();
    let bytes = write_file(&r, writer, !rest.is_empty() || case == "bigtiff", std::path::Path::new(path))?;
    let export_ms = start.elapsed().as_secs_f64() * 1000.0;
    println!("{}", serde_json::json!({"writer":writer,"case":case,"width":width,"height":height,"bytes":bytes,"export_ms":export_ms}));
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}
