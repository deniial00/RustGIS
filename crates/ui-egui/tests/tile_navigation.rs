//! Tile pictures survive camera changes; expensive basemap work never runs in a pan frame.
use rustgis_tiles::{cache::Payload, mvt};
use rustgis_ui_egui::RustGisApp;
use rustgis_ui_egui::rustgis_engine::{
    geom::{Coord, lonlat_to_merc},
    render::{self, rustgis_tiles},
};
use serde_json::json;

fn serve() {
    let tile = mvt::Tile {
        layers: vec![mvt::Layer {
            name: "blocks".into(),
            extent: 4096,
            features: vec![mvt::Feature {
                id: Some(1),
                kind: mvt::GeomKind::Polygon,
                geometry: vec![vec![[300.0, 300.0], [3200.0, 300.0], [3200.0, 3200.0], [300.0, 3200.0], [300.0, 300.0]]],
                props: vec![],
            }],
        }],
    };
    let tile = mvt::encode(&tile);
    let style = json!({"version": 8,
        "sources": {"v": {"type": "vector", "tiles": ["https://navigation.test/{z}/{x}/{y}.pbf"]}},
        "layers": [
            {"id": "background", "type": "background", "paint": {"background-color": "#ffffff"}},
            {"id": "blocks", "type": "fill", "source": "v", "source-layer": "blocks", "paint": {"fill-color": "#d03020"}}
        ]
    });
    let style = serde_json::to_vec(&style).expect("style");
    let mut pm = render::Pixmap::new(256, 256).expect("pixmap");
    pm.data_mut().as_chunks_mut::<4>().0.fill([208, 48, 32, 255]);
    let png = pm.encode_png().expect("PNG");
    rustgis_tiles::cache::set_fetcher(move |url, done| {
        done(Ok(if url.ends_with(".json") {
            style.clone()
        } else if url.ends_with(".pbf") {
            tile.clone()
        } else {
            png.clone()
        }));
    });
}

fn settle(h: &mut egui_kittest::Harness<'_, RustGisApp>) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
    for _ in 0..4 {
        for _ in 0..30 {
            h.step();
        }
        while h.state().map.drawing() {
            assert!(std::time::Instant::now() < end, "basemap worker never finished");
            std::thread::sleep(std::time::Duration::from_millis(2));
            h.step();
        }
    }
}

fn render_state() -> Option<egui_wgpu::RenderState> {
    let setup = egui_wgpu::WgpuSetup::CreateNew(egui_wgpu::WgpuSetupCreateNew::without_display_handle());
    let instance = pollster::block_on(setup.new_instance());
    pollster::block_on(egui_wgpu::RenderState::create(
        &egui_wgpu::WgpuConfiguration { wgpu_setup: setup, ..Default::default() },
        &instance,
        None,
        egui_wgpu::RendererOptions::PREDICTABLE,
    ))
    .ok()
}

fn assert_gpu_matches_ground(h: &mut egui_kittest::Harness<'_, RustGisApp>) {
    assert!(h.state().map.moves_on_gpu(), "vector basemap uses tile textures");
    let image = h.render().expect("GPU picture");
    let rect = h.state().map.view_rect.expect("map rectangle").0;
    let view = render::View {
        width: rect.width() as f64,
        height: rect.height() as f64,
        pixels_per_point: 1.0,
        camera: h.state().session.map().camera,
        sky: true,
    };
    let expected = render::render(&mut h.state().session.map().clone(), &view).expect("CPU reference");
    let mut error = 0u64;
    let mut samples = 0;
    for y in (30..rect.height() as u32 - 30).step_by(7) {
        for x in (30..rect.width() as u32 - 30).step_by(7) {
            let a = image.get_pixel(rect.min.x.round() as u32 + x, rect.min.y.round() as u32 + y);
            let b = expected.pixmap.pixel(x, y).expect("pixel");
            error += u64::from(a[0].abs_diff(b.red())) + u64::from(a[1].abs_diff(b.green()));
            samples += 2;
        }
    }
    assert!(error < samples * 5, "moving tile textures sample the wrong ground: mean color error {}", error / samples);
}

#[test]
fn vector_tiles_are_reused_and_cpu_pan_gaps_are_composed_off_thread() {
    serve();
    let mut map = rustgis_ui_egui::rustgis_engine::map::Map::new("Tiles");
    map.graticule = false;
    map.tiles.push(rustgis_ui_egui::rustgis_engine::map::TileLayer::new(
        "Vector",
        rustgis_ui_egui::rustgis_engine::map::TileKind::Style,
        "https://navigation.test/style.json",
    ));
    let view = render::View {
        width: 300.0,
        height: 200.0,
        pixels_per_point: 1.0,
        camera: rustgis_ui_egui::rustgis_engine::map::Camera {
            center: lonlat_to_merc(Coord::new(2.34, 48.86)),
            res: 10.0,
            bearing: 0.0,
            pitch: 0.0,
        },
        sky: false,
    };
    render::render_png(&mut map, &view).expect("load tiles");
    let pictures = |map: &mut _, view: &_| {
        let Some(render::RenderOutput::Gpu(scene)) = render::render_backend(map, view, render::Backend::Gpu) else { panic!("GPU scene") };
        assert!(
            !scene.cmds.iter().any(|c| matches!(c, render::gpu_scene::DrawCmd::Image { .. })),
            "vector basemaps must move as reusable tiles, not a newly painted view"
        );
        scene
            .cmds
            .into_iter()
            .filter_map(|c| match c {
                render::gpu_scene::DrawCmd::Tiles { tiles, .. } => Some(tiles),
                _ => None,
            })
            .flatten()
            .filter(|q| matches!(&*q.picture, Payload::Image(p) if p.width() > 1))
            .map(|q| q.picture)
            .collect::<Vec<_>>()
    };
    let first = pictures(&mut map, &view);
    let mut pan = view;
    pan.camera.center.x += 100.0;
    let next = pictures(&mut map, &pan);
    assert!(!first.is_empty());
    assert!(next.iter().any(|b| first.iter().any(|a| std::sync::Arc::ptr_eq(a, b))), "a pan must reuse painted tiles");

    for (vector, gpu) in [(false, false), (true, false), (true, true)] {
        let rs = if gpu {
            let Some(rs) = render_state() else { continue };
            Some(rs)
        } else {
            None
        };
        let mut app = RustGisApp::with_sample();
        app.set_render_state(rs.clone());
        app.run("project.new", json!({"name": "Navigation"})).expect("project");
        app.run("ui.map.gpu", json!({"on": gpu})).expect("backend");
        app.run("map.addTiles", json!({"url": if vector { "https://navigation.test/style.json" } else { "https://navigation.test/{z}/{x}/{y}.png" }, "replace": true})).expect("basemap");
        app.session.map_mut().camera = view.camera;
        let mut builder =
            egui_kittest::Harness::builder().with_size(egui::vec2(900.0, 550.0)).with_step_dt(1.0 / 60.0).with_max_steps(10_000);
        if let Some(rs) = rs {
            builder = builder.renderer(egui_kittest::wgpu::WgpuTestRenderer::from_render_state(rs));
        }
        let mut h = builder.build_ui_state(
            |ui, app: &mut RustGisApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        settle(&mut h);
        if gpu {
            assert_gpu_matches_ground(&mut h);
        }
        let before = h.state().map.full_frame_camera();
        let from = egui::pos2(550.0, 350.0);
        h.input_mut().events.extend([
            egui::Event::PointerMoved(from),
            egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
        ]);
        h.step();
        h.input_mut().events.push(egui::Event::PointerMoved(from + egui::vec2(90.0, 0.0)));
        h.step();
        assert_ne!(h.state().session.map().camera, before, "pan engaged");
        assert_eq!(h.state().map.full_frame_camera(), before, "a pan gap must not synchronously paint the basemap");
        if gpu {
            assert_gpu_matches_ground(&mut h);
        } else {
            assert!(h.state().map.drawing(), "a worker fills the pan gap");
        }
        let pos = from + egui::vec2(90.0, 0.0);
        h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        settle(&mut h);
        assert_eq!(h.state().map.full_frame_camera(), h.state().session.map().camera, "settling draws the exact final camera");
    }
}
