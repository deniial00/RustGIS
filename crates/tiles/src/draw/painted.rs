//! Bounded, shared cache of painted vector tiles. Neither camera position nor the clip of a
//! parent standing in for a child belongs in the painting: panning and stand-ins reuse it.
use super::*;
use std::ops::Range;
use std::sync::{Mutex, OnceLock, Weak};

use crate::source::Resolved;

const BUDGET: usize = if cfg!(target_arch = "wasm32") { 48 << 20 } else { 128 << 20 };
const MAX_SIDE: u32 = 2048;
const MAX_ENTRIES: usize = 2048;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    style: usize,
    tile: usize,
    sprite: usize,
    start: usize,
    end: usize,
    zoom: u32,
    z: u8,
    // Patterns are anchored to the world, including across copies of it.
    x: u64,
    y: u64,
}

struct Entry {
    style: Weak<Resolved>,
    tile: Weak<Payload>,
    sprite: Option<Weak<Sprite>>,
    picture: Arc<Payload>,
    side: u32,
    used: u64,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<Key, Entry>,
    bytes: usize,
    tick: u64,
}

fn cache() -> &'static Mutex<Cache> {
    static C: OnceLock<Mutex<Cache>> = OnceLock::new();
    C.get_or_init(Mutex::default)
}

pub(super) fn forget() {
    *cache().lock().unwrap_or_else(|e| e.into_inner()) = Cache::default();
}

/// A complete tile, at the requested zoom and pixel density. Higher-density pictures of
/// the same zoom also serve previews. Large overzooms use the original vector path instead.
pub(super) fn tile(
    r: &Arc<Resolved>,
    it: &Item,
    layers: Range<usize>,
    frame: &Frame,
    zoom: f32,
    sheet: Option<&Arc<Sprite>>,
) -> Option<Arc<Payload>> {
    let span = TileId::span(it.z);
    let size = (span / frame.m_per_px).ceil();
    if !(1.0..=f64::from(MAX_SIDE)).contains(&size) || !matches!(&*it.payload, Payload::Vector(_)) {
        return None;
    }
    let side = size as u32;
    let key = Key {
        style: Arc::as_ptr(r) as usize,
        tile: Arc::as_ptr(&it.payload) as usize,
        sprite: sheet.map_or(0, |s| Arc::as_ptr(s) as usize),
        start: layers.start,
        end: layers.end,
        zoom: zoom.to_bits(),
        z: it.z,
        x: it.at.min_x.to_bits(),
        y: it.at.max_y.to_bits(),
    };
    {
        let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
        c.tick = c.tick.wrapping_add(1);
        let tick = c.tick;
        if let Some(e) = c.entries.get_mut(&key)
            && e.side >= side
        {
            e.used = tick;
            return Some(e.picture.clone());
        }
    }
    // No global lock is held during rasterization. A frame worker must never block the UI
    // thread reading an already painted tile.
    let mut pm = Pixmap::new(side, side)?;
    let m_per_px = span / f64::from(side);
    let local = Frame { left: it.at.min_x, top: it.at.max_y, m_per_px, ppp: (frame.m_per_px * f64::from(frame.ppp) / m_per_px) as f32 };
    // Buffered features just outside a tile can still paint into it (a road's stroke, for
    // example). Clip geometry beyond the widest supported stroke's miter, and let the
    // pixmap clip its pixels at the actual tile boundary.
    let pad = 1024.0 * m_per_px;
    let clip = Extent::new(it.at.min_x - pad, it.at.min_y - pad, it.at.max_x + pad, it.at.max_y + pad);
    let whole = Item { clip, ..it.clone() };
    for layer in r.style.layers.get(layers).into_iter().flatten() {
        if layer.visible_at(zoom) && drawable(layer) && (pattern(layer).is_none() || sheet.is_some()) {
            draw_vector(&mut pm, &whole, layer, &local, zoom, sheet.map(Arc::as_ref));
        }
    }
    let picture = Arc::new(Payload::Image(pm));
    let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(e) = c.entries.get(&key)
        && e.side >= side
    {
        return Some(e.picture.clone());
    }
    // Weak references prevent pointer reuse from confusing two styles, tiles or sprites,
    // without extending the lifetime of their decoded source data.
    let mut expired_bytes = 0;
    c.entries.retain(|_, e| {
        let live = e.style.strong_count() > 0 && e.tile.strong_count() > 0 && e.sprite.as_ref().is_none_or(|s| s.strong_count() > 0);
        if !live {
            expired_bytes += e.side as usize * e.side as usize * 4;
        }
        live
    });
    c.bytes = c.bytes.saturating_sub(expired_bytes);
    let entry = Entry {
        style: Arc::downgrade(r),
        tile: Arc::downgrade(&it.payload),
        sprite: sheet.map(Arc::downgrade),
        picture: picture.clone(),
        side,
        used: c.tick,
    };
    if let Some(old) = c.entries.insert(key, entry) {
        c.bytes = c.bytes.saturating_sub(old.side as usize * old.side as usize * 4);
    }
    c.bytes += side as usize * side as usize * 4;
    while c.bytes > BUDGET || c.entries.len() > MAX_ENTRIES {
        let Some(oldest) = c.entries.iter().min_by_key(|(_, e)| e.used).map(|(k, _)| *k) else { break };
        if let Some(e) = c.entries.remove(&oldest) {
            c.bytes = c.bytes.saturating_sub(e.side as usize * e.side as usize * 4);
        }
    }
    Some(picture)
}
