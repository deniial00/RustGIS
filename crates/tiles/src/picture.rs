//! A style's vector shapes drawn once into tile pictures.
//!
//! Filling and stroking the thousands of paths in a vector tile is nearly all the cost of
//! drawing a style, and a map moving across its tiles would draw them all again every frame. So
//! a style's shapes (every layer but its labels) are drawn square by square into pictures, kept
//! here, and put in place like raster tiles: copied into the map on the CPU, a texture each moved
//! with the camera on the GPU. Labels aren't in them: they are collected every time, for the
//! renderer to place with the map's own.
//!
//! A picture is drawn at the map's zoom rounded to a [`STEPS`]th of a level and its side to whole
//! pixels, so it is put in place stretched by a fraction of a percent at most (and copied pixel
//! for pixel). Its square is a tile a level or more below the vector tiles drawn there: 256 to
//! 512 pixels a side at one or two pixels a point, about as much at others (192 to 384 at 1.5).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};

use rustgis_geom::Extent;
use tiny_skia::Pixmap;

use crate::cache::{self, Payload};
use crate::draw::{Drawn, Frame, Item, Pass, draw_layers, overhang};
use crate::source::Resolved;
use crate::style::{LayerKind, Source};

use crate::template::{self, Placed, TileId};

pub mod worker;

/// The zooms a level a picture is drawn at.
const STEPS: f32 = 256.0;

/// The most picture memory kept, bytes.
const BUDGET: usize = if cfg!(target_arch = "wasm32") { 96 << 20 } else { 384 << 20 };

/// The largest side of a picture, pixels.
const MAX_SIDE: f32 = 2048.0;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    /// The resolved style (its address; [`Entry::style`] tells whether it is still the same).
    style: usize,
    tile: TileId,
    /// The zoom, in [`STEPS`].
    step: i32,
    /// Pixels per point.
    ppp: u32,
}

struct Entry {
    style: Weak<Resolved>,
    picture: Arc<Payload>,
    bytes: usize,
    used: u64,
    /// Drawn while some of it was on its way, at this cache generation: drawn again once the
    /// cache has more.
    waiting: Option<u64>,
}

#[derive(Default)]
struct Store {
    entries: HashMap<Key, Entry>,
    bytes: usize,
    /// Counts the calls to [`cover`], for the least recently used.
    tick: u64,
    pending: HashMap<Key, (u64, u64)>,
    failed: HashSet<usize>,
    epoch: u64,
    ticket: u64,
}

fn lock() -> MutexGuard<'static, Store> {
    static S: OnceLock<Mutex<Store>> = OnceLock::new();
    S.get_or_init(Mutex::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// Forgets every picture (with [`cache::clear`]).
pub(crate) fn forget() {
    let mut s = lock();
    s.entries.clear();
    s.bytes = 0;
    s.pending.clear();
    s.failed.clear();
    s.epoch = s.epoch.wrapping_add(1);
}

/// Whether the style draws shapes from vector tiles at `zoom`.
fn draws_shapes(r: &Resolved, zoom: f32) -> bool {
    r.style.layers.iter().any(|l| {
        matches!(l.kind, LayerKind::Fill | LayerKind::FillExtrusion | LayerKind::Line | LayerKind::Circle)
            && l.visible_at(zoom)
            && l.source.as_deref().is_some_and(|id| r.style.sources.iter().any(|(s, src)| s == id && matches!(src, Source::Vector { .. })))
    })
}

/// The pictures of `r`'s shapes over `view`, drawing those missing, and out to `reach` (only
/// those already drawn), bottom first; `None` when the style draws no vector shapes here, or the
/// view is too wide for pictures (the whole world on the globe). `frame` gives the scale.
pub(crate) fn cover(
    spec: &crate::Spec,
    r: &Arc<Resolved>,
    frame: &Frame,
    view: &Extent,
    reach: Option<&Extent>,
    pending: &mut bool,
) -> Option<Vec<Item>> {
    let ppp = frame.ppp;
    if !(ppp.is_finite() && ppp > 0.0 && frame.m_per_px.is_finite() && frame.m_per_px > 0.0) || view.width() > template::HALF_WORLD {
        return None;
    }
    let zoom = template::style_zoom(frame.m_per_px * f64::from(ppp));
    if !draws_shapes(r, zoom) {
        return None;
    }
    let step = (zoom * STEPS).round();
    let level = (step / STEPS).floor();
    // Squares of 256 to 512 pixels: those the size of a 256-point tile at one pixel a point, of
    // a 128-point tile at two, and so on. Smaller ones would each look through more tiles'
    // features; larger ones reach further past the view.
    let down = 1.0 + ppp.log2().round().clamp(0.0, 3.0);
    let z = (level + down) as u8;
    let side = (512.0 * ppp * (step / STEPS - level - down).exp2()).round().clamp(16.0, MAX_SIDE) as u32;
    let m_per_px = TileId::span(z) / f64::from(side);
    let key = |tile| Key { style: Arc::as_ptr(r) as usize, tile, step: step as i32, ppp: ppp.to_bits() };
    let generation = cache::generation();
    // A view of more squares than a covering holds (a huge window) is drawn straight instead;
    // a reach of more keeps to the view.
    let span = TileId::span(z);
    if ((view.width() / span).ceil() + 1.0) * ((view.height() / span).ceil() + 1.0) > template::MAX_TILES as f64 {
        return None;
    }
    let squares = reach
        .map(|e| template::covering(e, z))
        .filter(|s| !s.is_empty() && s.len() < template::MAX_TILES)
        .unwrap_or_else(|| template::covering(view, z));
    let mut found: Vec<(Placed, Option<Arc<Payload>>)> = Vec::with_capacity(squares.len());
    let mut missing = Vec::new();
    {
        let mut s = lock();
        s.tick += 1;
        let tick = s.tick;
        for p in squares {
            let in_view = p.extent().intersects(view);
            let e = s.entries.get_mut(&key(p.id)).filter(|e| e.style.upgrade().is_some_and(|x| Arc::ptr_eq(&x, r)));
            match e {
                Some(e) if !in_view || e.waiting.is_none_or(|g| g == generation) => {
                    e.used = tick;
                    *pending |= in_view && e.waiting.is_some();
                    found.push((p, Some(e.picture.clone())));
                }
                _ if in_view => {
                    missing.push(found.len());
                    found.push((p, None));
                }
                _ => {}
            }
        }
    }
    let todo: Vec<Placed> = missing.iter().filter_map(|&i| found.get(i).map(|f| f.0)).collect();
    // How far past its square a picture looks for shapes, in points: a line or circle drawn from
    // a neighbouring tile reaches into it by up to half its width.
    let reach = r.style.layers.iter().filter(|l| l.visible_at(zoom)).map(|l| overhang(l, zoom)).fold(0.0f32, f32::max);
    let deferred = worker::enqueue(spec, r, &todo, &key, side, m_per_px, ppp, pending);
    let drawn = if deferred { Vec::new() } else { draw_all(r, &todo, side, m_per_px, ppp, f64::from(reach)) };
    {
        let mut s = lock();
        let tick = s.tick;
        for (&i, d) in missing.iter().zip(drawn) {
            let (Some(slot), Some((pm, waiting))) = (found.get_mut(i), d) else { continue };
            *pending |= waiting;
            let bytes = pm.data().len();
            let picture = Arc::new(Payload::Image(pm));
            let entry =
                Entry { style: Arc::downgrade(r), picture: picture.clone(), bytes, used: tick, waiting: waiting.then_some(generation) };
            s.bytes += bytes;
            if let Some(old) = s.entries.insert(key(slot.0.id), entry) {
                s.bytes = s.bytes.saturating_sub(old.bytes);
            }
            slot.1 = Some(picture);
        }
        // Only when pictures were added: a frame of pictures already drawn costs nothing here.
        if !missing.is_empty() {
            evict(&mut s);
        }
    }
    let s = lock();
    Some(
        found
            .into_iter()
            .flat_map(|(p, picture)| match picture {
                Some(payload) => vec![Item { payload, at: p.extent(), z, clip: p.extent() }],
                None => worker::fallback(&s, r, &key(p.id), &p),
            })
            .collect(),
    )
}

/// Drops the least recently used pictures past the budget, and those of styles gone. Those
/// used by the latest call stay, whatever they weigh.
fn evict(s: &mut Store) {
    s.entries.retain(|_, e| e.style.strong_count() > 0);
    s.bytes = s.entries.values().map(|e| e.bytes).sum();
    if s.bytes <= BUDGET {
        return;
    }
    let tick = s.tick;
    let mut old: Vec<(u64, Key, usize)> = s.entries.iter().filter(|(_, e)| e.used != tick).map(|(k, e)| (e.used, *k, e.bytes)).collect();
    old.sort_unstable_by_key(|o| o.0);
    for (_, k, bytes) in old {
        if s.bytes <= BUDGET {
            break;
        }
        s.entries.remove(&k);
        s.bytes = s.bytes.saturating_sub(bytes);
    }
}

/// Draws the pictures of `squares`: each a `side`-pixel pixmap and whether some of it is still
/// on its way. Natively the squares are shared among threads.
fn draw_all(r: &Resolved, squares: &[Placed], side: u32, m_per_px: f64, ppp: f32, reach: f64) -> Vec<Option<(Pixmap, bool)>> {
    let one = |p: &Placed| draw_one(r, p, side, m_per_px, ppp, reach);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(squares.len()).min(16);
        if threads > 1 {
            let next = std::sync::atomic::AtomicUsize::new(0);
            let out: Mutex<Vec<Option<(Pixmap, bool)>>> = Mutex::new((0..squares.len()).map(|_| None).collect());
            std::thread::scope(|scope| {
                for _ in 0..threads {
                    scope.spawn(|| {
                        loop {
                            let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some(p) = squares.get(i) else { break };
                            let d = one(p);
                            if let Some(slot) = out.lock().unwrap_or_else(|e| e.into_inner()).get_mut(i) {
                                *slot = d;
                            }
                        }
                    });
                }
            });
            return out.into_inner().unwrap_or_else(|e| e.into_inner());
        }
    }
    squares.iter().map(one).collect()
}

/// Draws the shapes of one square, with those of the tiles around it out to `reach` points.
fn draw_one(r: &Resolved, p: &Placed, side: u32, m_per_px: f64, ppp: f32, reach: f64) -> Option<(Pixmap, bool)> {
    let mut pm = Pixmap::new(side, side)?;
    let e = p.extent();
    let frame = Frame { left: e.min_x, top: e.max_y, m_per_px, ppp };
    // Shapes from the tiles next to it too, where they reach into it.
    let pad = reach * f64::from(ppp) * m_per_px;
    let view = Extent::new(e.min_x - pad, e.min_y - pad, e.max_x + pad, e.max_y + pad);
    let mut d = Drawn::default();
    draw_layers(Some(&mut pm), (side, side), &view, r, &frame, Pass::Shapes, &mut d);
    Some((pm, d.pending))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::{draw, place};
    use crate::mvt::{Feature, GeomKind, Layer, Tile};
    use crate::source::{Kind, Resolve, Spec, resolve};

    /// A one-tile archive (zoom 0) of a park, two roads (one dashed) and a point, with a style
    /// drawing them all, so a view at any zoom draws the tile.
    fn spec(name: &str) -> (Spec, std::path::PathBuf) {
        let feature = |kind, geometry: Vec<Vec<[f32; 2]>>, class: &str| Feature {
            id: None,
            kind,
            geometry,
            props: vec![("class".into(), crate::mvt::Value::String(class.into()))],
        };
        let tile = Tile {
            layers: vec![
                Layer {
                    name: "parks".into(),
                    extent: 4096,
                    features: vec![feature(
                        GeomKind::Polygon,
                        vec![vec![[1000.0, 1000.0], [3000.0, 1100.0], [2900.0, 3000.0], [1100.0, 2900.0], [1000.0, 1000.0]]],
                        "park",
                    )],
                },
                Layer {
                    name: "roads".into(),
                    extent: 4096,
                    features: vec![
                        feature(GeomKind::LineString, vec![vec![[0.0, 300.0], [4096.0, 3900.0]]], "major"),
                        feature(GeomKind::LineString, vec![vec![[200.0, 3800.0], [2000.0, 2100.0], [3900.0, 200.0]]], "path"),
                    ],
                },
                Layer { name: "pois".into(), extent: 4096, features: vec![feature(GeomKind::Point, vec![vec![[2048.0, 2048.0]]], "cafe")] },
            ],
        };
        let dir = std::env::temp_dir().join(format!("rustgis-picture-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.pmtiles");
        let new = crate::pmtiles::NewArchive {
            tile_type: crate::pmtiles::TileType::Mvt,
            gzipped_tiles: false,
            bounds: [-180.0, -85.0, 180.0, 85.0],
            metadata: serde_json::json!({"vector_layers": [{"id": "parks"}, {"id": "roads"}, {"id": "pois"}]}),
        };
        std::fs::write(&path, crate::pmtiles::write(vec![(0, 0, 0, crate::mvt::encode(&tile))], &new).unwrap()).unwrap();
        let style = r##"[
            {"id": "bg", "type": "background", "paint": {"background-color": "#f0eee8"}},
            {"id": "park", "type": "fill", "source": "x", "source-layer": "parks", "paint": {"fill-color": "#9c9", "fill-outline-color": "#363"}},
            {"id": "major", "type": "line", "source": "x", "source-layer": "roads", "filter": ["==", "class", "major"],
                "layout": {"line-join": "round"}, "paint": {"line-color": "#e92", "line-width": 9}},
            {"id": "path", "type": "line", "source": "x", "source-layer": "roads", "filter": ["==", "class", "path"],
                "paint": {"line-color": "#333", "line-width": 2, "line-dasharray": [3, 2]}},
            {"id": "poi", "type": "circle", "source": "x", "source-layer": "pois", "paint": {"circle-color": "#c00", "circle-radius": 7}},
            {"id": "label", "type": "symbol", "source": "x", "source-layer": "pois", "layout": {"text-field": ["get", "class"]}}
        ]"##;
        let spec = Spec {
            kind: Kind::PmTiles,
            url: path.display().to_string(),
            subdomains: Vec::new(),
            min_zoom: 0,
            max_zoom: 14,
            tile_size: 512,
            tms: false,
            attribution: String::new(),
            layers: Vec::new(),
            style_layers: style.into(),
        };
        (spec, dir)
    }

    /// The style resolved, once its archive is read.
    fn resolved(spec: &Spec) -> Arc<Resolved> {
        for _ in 0..500 {
            if let Resolve::Ready(r) = resolve(spec) {
                return r;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the archive wasn't read");
    }

    /// A 300 × 200 point view at `zoom`, `ppp` pixels a point, with the middle of the world in
    /// it, on the pixels of the pictures (elsewhere they are put in place to the nearest pixel).
    fn frame(zoom: f64, ppp: f32) -> Frame {
        let half = template::HALF_WORLD;
        let m_per_px = 2.0 * half / (512.0 * zoom.exp2()) / f64::from(ppp);
        let on_pixels = |px: f64| ((half - px * m_per_px) / m_per_px).round() * m_per_px;
        Frame { left: on_pixels(195.0 * f64::from(ppp)) - half, top: half - on_pixels(70.0 * f64::from(ppp)), m_per_px, ppp }
    }

    /// Draws `spec` until all of it has arrived.
    fn drawn(spec: &Spec, f: &Frame, ppp: f32) -> (Pixmap, crate::draw::Drawn) {
        let (w, h) = ((300.0 * ppp) as u32, (200.0 * ppp) as u32);
        for _ in 0..500 {
            let mut pm = Pixmap::new(w, h).unwrap();
            let d = draw(&mut pm, spec, 1.0, f, true);
            if !d.pending {
                return (pm, d);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("still pending");
    }

    #[test]
    fn pictures_draw_what_the_style_draws() {
        let (spec, dir) = spec("same");
        let r = resolved(&spec);
        // Zooms whose pictures are a whole number of pixels across, so they are at the view's
        // own scale (in between, a picture is a hair larger or smaller: a long dashed line's
        // dashes then fall a pixel or two along).
        for (zoom, ppp) in [(1.0, 1.0), (2.0, 2.0), (4.0, 1.5), (3.0, 1.0)] {
            let f = frame(zoom, ppp);
            let (pm, d) = drawn(&spec, &f, ppp);
            // As the style drawn straight into the view, but for anti-aliasing where the pictures meet.
            let mut direct = Pixmap::new(pm.width(), pm.height()).unwrap();
            let mut nothing = crate::draw::Drawn::default();
            draw_layers(
                Some(&mut direct),
                (pm.width(), pm.height()),
                &f.extent(pm.width(), pm.height()),
                &r,
                &f,
                Pass::Shapes,
                &mut nothing,
            );
            let diff: Vec<u8> = pm.data().iter().zip(direct.data()).map(|(a, b)| a.abs_diff(*b)).collect();
            let off = diff.chunks(4).filter(|p| p.iter().any(|d| *d > 40)).count();
            assert!(off * 400 < diff.len() / 4, "zoom {zoom}: {off} pixels differ");
            // Each colour is there: the park, both roads, the point.
            for c in [[0x99, 0xcc, 0x99], [0xee, 0x99, 0x22], [0x33, 0x33, 0x33], [0xcc, 0x00, 0x00]] {
                assert!(pm.pixels().iter().any(|p| [p.red(), p.green(), p.blue()] == c), "zoom {zoom}: no {c:?}");
            }
            // The labels still come out, on their own.
            assert_eq!(d.labels.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["cafe"], "zoom {zoom}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn background_pictures_are_deferred_reused_bounded_and_ignore_obsolete_answers() {
        if std::env::var_os("RUSTGIS_TEST_TILE_WORKER").is_none() {
            let out = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "picture::tests::background_pictures_are_deferred_reused_bounded_and_ignore_obsolete_answers",
                    "--nocapture",
                ])
                .env("RUSTGIS_TEST_TILE_WORKER", "1")
                .output()
                .unwrap();
            let text = String::from_utf8_lossy(&out.stdout);
            assert!(out.status.success(), "{text}\n{}", String::from_utf8_lossy(&out.stderr));
            assert!(text.contains("1 passed"), "isolated test did not run");
            return;
        }
        #[derive(Default)]
        struct Host {
            jobs: Mutex<Vec<(worker::Request, worker::Done)>>,
        }
        impl worker::Painter for Host {
            fn paint(&self, request: worker::Request, done: worker::Done) {
                self.jobs.lock().unwrap().push((request, done));
            }
            fn cancel(&self, id: u64) {
                let old = {
                    let mut jobs = self.jobs.lock().unwrap();
                    jobs.iter().position(|(r, _)| r.id == id).map(|i| jobs.remove(i))
                };
                if let Some((_, done)) = old {
                    done(Ok(None));
                }
            }
        }
        let (spec, dir) = spec("background");
        let r = resolved(&spec);
        let host = Arc::new(Host::default());
        worker::set_painter(Some(host.clone()));
        let f = frame(2.2, 1.0);
        let view = f.extent(300, 200);
        let first = place(&spec, 1.0, &f, &view, &view, None).unwrap();
        assert!(first.pending && first.tiles.is_empty(), "a missing picture must not be painted on the caller");
        let jobs = std::mem::take(&mut *host.jobs.lock().unwrap());
        assert!(!jobs.is_empty());
        for (request, done) in jobs {
            let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let pixels = loop {
                if let Some(pm) = worker::draw(&request.spec, &request.frame, request.side).unwrap() {
                    break pm;
                }
                assert!(std::time::Instant::now() < end, "archive did not finish loading");
                std::thread::sleep(std::time::Duration::from_millis(2));
            };
            done(Ok(Some(pixels)));
        }
        let ready = place(&spec, 1.0, &f, &view, &view, None).unwrap();
        assert!(!ready.pending && !ready.tiles.is_empty());
        let zoom = Frame { m_per_px: f.m_per_px / 1.03, ..f };
        let view = zoom.extent(300, 200);
        let waiting = place(&spec, 1.0, &zoom, &view, &view, None).unwrap();
        assert!(waiting.pending && !waiting.tiles.is_empty(), "the previous zoom stands in while painting");
        assert!(waiting.tiles.iter().all(|p| ready.tiles.iter().any(|old| Arc::ptr_eq(&p.picture, &old.picture))));
        let n = host.jobs.lock().unwrap().len();
        place(&spec, 1.0, &zoom, &view, &view, None);
        assert_eq!(host.jobs.lock().unwrap().len(), n, "pending work is coalesced");
        for i in 1..100 {
            let f = frame(2.2 + f64::from(i) / 100.0, 1.0);
            let e = f.extent(300, 200);
            place(&spec, 1.0, &f, &e, &e, None);
            assert!(host.jobs.lock().unwrap().len() <= 64, "obsolete zoom jobs grow without bound");
        }
        let old = std::mem::take(&mut *host.jobs.lock().unwrap());
        cache::clear();
        for (request, done) in old {
            done(Ok(Pixmap::new(request.side, request.side)));
        }
        assert!(lock().entries.is_empty(), "clearing the cache must invalidate outstanding answers");
        worker::set_painter(None);
        drop(r);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn pending_picture_fallbacks_use_one_density_and_clip_child_tiles() {
        let (spec, dir) = spec("fallback-clips");
        let r = resolved(&spec);
        let style = Arc::as_ptr(&r) as usize;
        let mut store = Store::default();
        for (z, ppp) in [(3, 1.0_f32), (4, 2.0_f32)] {
            let factor = 1u32 << (z - 2);
            for y in 0..factor {
                for x in 0..factor {
                    let k = Key { style, tile: TileId { z, x: 2 * factor + x, y: factor + y }, step: 515, ppp: ppp.to_bits() };
                    let pm = Pixmap::new(1, 1).unwrap();
                    store.entries.insert(
                        k,
                        Entry { style: Arc::downgrade(&r), picture: Arc::new(Payload::Image(pm)), bytes: 4, used: 0, waiting: None },
                    );
                }
            }
        }
        let cell = Placed { id: TileId { z: 2, x: 2, y: 1 }, wx: 2 };
        let wanted = Key { style, tile: cell.id, step: 510, ppp: 1.5_f32.to_bits() };
        let fallback = worker::fallback(&store, &r, &wanted, &cell);
        assert_eq!(fallback.len(), 16, "only the closer 2x density is drawn");
        let mut area = 0.0;
        for item in &fallback {
            assert!(item.clip.min_x >= item.at.min_x && item.clip.max_x <= item.at.max_x);
            assert!(item.clip.min_y >= item.at.min_y && item.clip.max_y <= item.at.max_y);
            area += item.clip.width() * item.clip.height();
        }
        let e = cell.extent();
        assert!((area / (e.width() * e.height()) - 1.0).abs() < 1e-12, "fallbacks must cover the cell once");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn worker_pixels_and_dimensions_are_checked_before_allocation() {
        assert!(worker::from_pixels(0, Vec::new()).is_none());
        assert!(worker::from_pixels(u32::MAX, Vec::new()).is_none());
        assert!(worker::from_pixels(2, vec![0; 15]).is_none());
        assert!(worker::from_pixels(2, vec![0; 16]).is_some());
        let (spec, dir) = spec("bad-worker-frame");
        for m_per_px in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let frame = Frame { left: 0.0, top: 0.0, m_per_px, ppp: 1.0 };
            assert!(worker::draw(&spec, &frame, 256).is_err());
        }
        assert!(worker::draw(&spec, &frame(2.0, 1.0), u32::MAX).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn pictures_are_drawn_once() {
        let (spec, dir) = spec("once");
        resolved(&spec);
        let f = frame(2.2, 1.0);
        drawn(&spec, &f, 1.0);
        let view = f.extent(300, 200);
        let placed = || place(&spec, 1.0, &f, &view, &view, None).unwrap().tiles;
        let (a, b) = (placed(), placed());
        assert!(!a.is_empty());
        assert!(a.iter().zip(&b).all(|(a, b)| Arc::ptr_eq(&a.picture, &b.picture)), "the same pictures");
        // A step along keeps those still in view, and the labels come with them.
        let moved = Frame { left: f.left + 90.0 * f.m_per_px, ..f };
        let p = place(&spec, 1.0, &moved, &moved.extent(300, 200), &moved.extent(300, 200), Some((300, 200))).unwrap();
        assert!(p.tiles.iter().any(|t| a.iter().any(|o| Arc::ptr_eq(&o.picture, &t.picture))));
        assert_eq!(p.labels.len(), 1);
        // Wider than the world (the globe's mosaic) or a broken scale: drawn straight instead.
        let r = resolved(&spec);
        let mut pending = false;
        let world = Extent::new(-template::HALF_WORLD * 1.5, -1e6, template::HALF_WORLD * 1.5, 1e6);
        assert!(cover(&spec, &r, &f, &world, None, &mut pending).is_none());
        // More squares than a covering holds: drawn straight too, not left blank.
        let near = frame(12.0, 1.0);
        let huge = near.extent(20_000, 10_000);
        assert!(huge.width() < template::HALF_WORLD);
        assert!(cover(&spec, &r, &near, &huge, None, &mut pending).is_none());
        for m_per_px in [0.0, f64::NAN, f64::INFINITY] {
            assert!(cover(&spec, &r, &Frame { m_per_px, ..f }, &view, None, &mut pending).is_none());
        }
        assert!(cover(&spec, &r, &Frame { ppp: f32::NAN, ..f }, &view, None, &mut pending).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
