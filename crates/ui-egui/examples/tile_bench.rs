//! Repeatable UI-thread pan/zoom benchmark with flat vector/raster basemaps served from memory.
//! GPU presentation/readback is excluded. Run with an optional maximum UI-frame cost:
//! `cargo run -p rustgis-ui-egui --example tile_bench -- /path/to/tile.pbf 50`.
//! The supplied MVT is replayed at every address using the Liberty style, without network I/O.
//! An optional fourth argument saves CPU/GPU pictures for visual inspection.
use rustgis_ui_egui::RustGisApp;
use rustgis_ui_egui::rustgis_engine::{
    geom::{Coord, lonlat_to_merc},
    render::rustgis_tiles,
};
use serde_json::json;

fn settle(h: &mut egui_kittest::Harness<'_, RustGisApp>) -> Result<(), Box<dyn std::error::Error>> {
    for _ in 0..40 {
        h.step();
    }
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while h.state().map.drawing() {
        if std::time::Instant::now() >= end {
            return Err("frame worker did not finish".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
        h.step();
    }
    Ok(())
}

fn render_state() -> Result<egui_wgpu::RenderState, Box<dyn std::error::Error>> {
    let setup = egui_wgpu::WgpuSetup::CreateNew(egui_wgpu::WgpuSetupCreateNew::without_display_handle());
    let instance = pollster::block_on(setup.new_instance());
    Ok(pollster::block_on(egui_wgpu::RenderState::create(
        &egui_wgpu::WgpuConfiguration { wgpu_setup: setup, ..Default::default() },
        &instance,
        None,
        egui_wgpu::RendererOptions::PREDICTABLE,
    ))?)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_fn(256, 256, |x, y| image::Rgba([x as u8, y as u8, 90, 255])).write_to(&mut bytes, image::ImageFormat::Png)?;
    let tile = std::fs::read(std::env::args().nth(1).ok_or("usage: tile_bench tile.pbf [max_ms]")?)?;
    let mut style: serde_json::Value = serde_json::from_str(include_str!("../../tiles/testdata/liberty.json"))?;
    style["sources"] = json!({"openmaptiles": {"type": "vector", "tiles": ["https://bench.test/{z}/{x}/{y}.pbf"], "maxzoom": 14}});
    style.as_object_mut().ok_or("style object")?.remove("sprite");
    let style = serde_json::to_vec(&style)?;
    rustgis_tiles::cache::set_fetcher(move |url, done| {
        let bytes = if url.ends_with(".json") {
            style.clone()
        } else if url.ends_with(".pbf") {
            tile.clone()
        } else {
            bytes.get_ref().clone()
        };
        std::thread::spawn(move || done(Ok(bytes)));
    });
    let mut worst: f64 = 0.0;
    for kind in ["vector", "raster"] {
        for gpu in [false, true] {
            let rs = render_state()?;
            let mut app = RustGisApp::with_sample();
            app.set_render_state(Some(rs.clone()));
            app.run("project.new", json!({"name": "Tile benchmark"}))?;
            let url = if kind == "vector" { "https://bench.test/style.json" } else { "https://bench.test/{z}/{x}/{y}.png" };
            app.run("map.addTiles", json!({"url": url, "replace": true}))?;
            app.run("ui.map.gpu", json!({"on": gpu}))?;
            app.session.map_mut().layers.clear();
            app.session.map_mut().camera.center = lonlat_to_merc(Coord::new(2.34, 48.86));
            app.session.map_mut().camera.res = 7.0;
            let mut h = egui_kittest::Harness::builder()
                .with_size(egui::vec2(1480.0, 900.0))
                .with_pixels_per_point(2.0)
                .with_step_dt(1.0 / 60.0)
                .with_max_steps(10_000)
                .renderer(egui_kittest::wgpu::WgpuTestRenderer::from_render_state(rs.clone()))
                .build_ui_state(
                    |ui, app: &mut RustGisApp| {
                        let ctx = ui.ctx().clone();
                        ctx.input_mut(|i| i.max_texture_side = 8192);
                        app.logic(&ctx);
                        app.ui(ui);
                    },
                    app,
                );
            for _ in 0..5 {
                settle(&mut h)?;
            }
            if let Some(dir) = std::env::args().nth(3) {
                std::fs::create_dir_all(&dir)?;
                h.render()?.save(format!("{dir}/{kind}-gpu-{gpu}.png"))?;
            }
            let from = egui::pos2(850.0, 550.0);
            h.input_mut().events.push(egui::Event::PointerMoved(from));
            h.input_mut().events.push(egui::Event::PointerButton {
                pos: from,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            });
            h.step();
            let mut ms = Vec::new();
            for i in 1..=90 {
                h.input_mut().events.push(egui::Event::PointerMoved(from + egui::vec2(i as f32 * 2.0, i as f32 * 0.2)));
                let start = std::time::Instant::now();
                h.step();
                ms.push(start.elapsed().as_secs_f64() * 1000.0);
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
            h.input_mut().events.push(egui::Event::PointerButton {
                pos: from + egui::vec2(180.0, 18.0),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            });
            for _ in 0..20 {
                let start = std::time::Instant::now();
                h.step();
                ms.push(start.elapsed().as_secs_f64() * 1000.0);
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
            for _ in 0..90 {
                h.input_mut().events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 4.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: Default::default(),
                });
                let start = std::time::Instant::now();
                h.step();
                ms.push(start.elapsed().as_secs_f64() * 1000.0);
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
            ms.sort_by(f64::total_cmp);
            let max = ms.last().copied().unwrap_or(0.0);
            worst = worst.max(max);
            println!(
                "{kind:8} GPU={gpu}: mean {:.2}, p95 {:.2}, max {max:.2} ms/frame",
                ms.iter().sum::<f64>() / ms.len() as f64,
                ms.get(ms.len() * 95 / 100).copied().unwrap_or(0.0)
            );
            settle(&mut h)?;
            let mut toggle_ms = 0.0f64;
            for visible in [false, true, false, true] {
                h.state_mut().run("map.setTiles", json!({"index": 0, "visible": visible}))?;
                let start = std::time::Instant::now();
                h.step();
                toggle_ms = toggle_ms.max(start.elapsed().as_secs_f64() * 1000.0);
                settle(&mut h)?;
            }
            worst = worst.max(toggle_ms);
            println!("         layer visibility: max {toggle_ms:.2} ms to the first updated frame");
        }
    }
    if let Some(limit) = std::env::args().nth(2) {
        let limit: f64 = limit.parse()?;
        if worst >= limit {
            return Err(format!("navigation stalls for {worst:.2} ms (limit {limit:.2})").into());
        }
    }
    Ok(())
}
