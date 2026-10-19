//! Measures the real native window, including egui painting, GPU submission and frame pacing.
//! `cargo run --release -p rustgis --example tile_interaction_bench -- cpu|gpu blank|style_url|tile.pbf`
use rustgis_engine::{
    geom::{Coord, lonlat_to_merc},
    render::rustgis_tiles,
};
use rustgis_ui_egui::RustGisApp;
use serde_json::json;
use std::time::Instant;

struct Bench {
    app: RustGisApp,
    start: Instant,
    last: Instant,
    dragging: bool,
    frames: Vec<f64>,
    cpu: Vec<f64>,
    reported: bool,
    last_camera: rustgis_engine::map::Camera,
    last_scene: Instant,
    refresh: Vec<f64>,
    work: Vec<f64>,
}

fn stats(name: &str, samples: &mut [f64]) {
    samples.sort_by(f64::total_cmp);
    let n = samples.len();
    println!(
        "{name}: n={n}, mean={:.2}, p95={:.2}, max={:.2} ms, >33ms={}",
        samples.iter().sum::<f64>() / n.max(1) as f64,
        samples.get(n * 95 / 100).copied().unwrap_or(0.0),
        samples.last().copied().unwrap_or(0.0),
        samples.iter().filter(|&&x| x > 33.0).count()
    );
}

impl eframe::App for Bench {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, input: &mut egui::RawInput) {
        let t = self.start.elapsed().as_secs_f64();
        let from = egui::pos2(800.0, 500.0);
        if (5.0..9.0).contains(&t) {
            if !self.dragging {
                input.events.push(egui::Event::PointerMoved(from));
                input.events.push(egui::Event::PointerButton {
                    pos: from,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                });
                self.dragging = true;
            }
            input.events.push(egui::Event::PointerMoved(from + egui::vec2(((t - 5.0) * 2.0).sin() as f32 * 250.0, 0.0)));
        } else if self.dragging {
            input.events.push(egui::Event::PointerButton {
                pos: from,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            });
            self.dragging = false;
        }
        if (10.0..14.0).contains(&t) {
            input.events.push(egui::Event::PointerMoved(from));
            input.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, if t < 12.0 { 5.0 } else { -5.0 }),
                phase: egui::TouchPhase::Move,
                modifiers: Default::default(),
            });
        }
    }
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.app.set_render_state(frame.wgpu_render_state().cloned());
        self.app.logic(ctx);
        ctx.request_repaint();
    }
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let now = Instant::now();
        let t = self.start.elapsed().as_secs_f64();
        if (5.0..14.0).contains(&t) {
            self.frames.push(now.duration_since(self.last).as_secs_f64() * 1000.0);
            if let Some(cpu) = frame.info().cpu_usage {
                self.cpu.push(f64::from(cpu) * 1000.0);
            }
        }
        self.last = now;
        self.app.ui(ui);
        let camera = self.app.map.full_frame_camera();
        if camera != self.last_camera {
            if (10.0..14.0).contains(&t) && self.last_scene.duration_since(self.start).as_secs_f64() >= 10.0 {
                self.refresh.push(now.duration_since(self.last_scene).as_secs_f64() * 1000.0);
                self.work.push(self.app.map.render_ms as f64);
            }
            self.last_scene = now;
            self.last_camera = camera;
        }
        if t >= 16.0 && !self.reported {
            stats("Native frame intervals", &mut self.frames);
            stats("CPU including egui painting/submission", &mut self.cpu);
            stats("Zoom content refresh intervals", &mut self.refresh);
            stats("Zoom scene worker cost", &mut self.work);
            self.reported = true;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn main() -> eframe::Result {
    let gpu = std::env::args().nth(1).as_deref() == Some("gpu");
    let source = std::env::args().nth(2).unwrap_or_else(|| "https://tiles.openfreemap.org/styles/liberty".into());
    if source != "blank" && !source.starts_with("https://") {
        let tile = std::fs::read(&source).map_err(|e| eframe::Error::AppCreation(Box::new(e)))?;
        let mut style: serde_json::Value = serde_json::from_str(include_str!("../../../crates/tiles/testdata/liberty.json"))
            .map_err(|e| eframe::Error::AppCreation(Box::new(e)))?;
        style["sources"] = json!({"openmaptiles":{"type":"vector","tiles":["https://bench.test/{z}/{x}/{y}.pbf"],"maxzoom":14}});
        if let Some(o) = style.as_object_mut() {
            o.remove("sprite");
        }
        let bytes = serde_json::to_vec(&style).map_err(|e| eframe::Error::AppCreation(Box::new(e)))?;
        rustgis_tiles::cache::set_fetcher(move |url, done| {
            done(Ok(if url.ends_with(".json") { bytes.clone() } else { tile.clone() }));
        });
    }
    let mut app = RustGisApp::with_sample();
    app.run("project.new", json!({"name":"Native interaction benchmark"})).map_err(|e| eframe::Error::AppCreation(e.into()))?;
    app.run("ui.map.gpu", json!({"on":gpu})).map_err(|e| eframe::Error::AppCreation(e.into()))?;
    if source != "blank" {
        let url = if source.starts_with("https://") { &source } else { "https://bench.test/style.json" };
        app.run("map.addTiles", json!({"url":url,"replace":true})).map_err(|e| eframe::Error::AppCreation(e.into()))?;
    }
    app.session.map_mut().camera.center = lonlat_to_merc(Coord::new(-73.98, 40.75));
    app.session.map_mut().camera.res = 7.0;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("RustGIS interaction benchmark").with_inner_size([1480.0, 900.0]),
        ..Default::default()
    };
    println!("Native benchmark GPU={gpu} source={source}");
    eframe::run_native(
        "RustGIS interaction benchmark",
        options,
        Box::new(move |_| {
            let now = Instant::now();
            let last_camera = app.session.map().camera;
            Ok(Box::new(Bench {
                app,
                start: now,
                last: now,
                dragging: false,
                frames: Vec::new(),
                cpu: Vec::new(),
                reported: false,
                last_camera,
                last_scene: now,
                refresh: Vec::new(),
                work: Vec::new(),
            }))
        }),
    )
}
