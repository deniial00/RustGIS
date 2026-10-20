//! Optional background tile painting. The browser installs a Web Worker host; native/headless
//! callers retain the synchronous painter. Completed pictures enter the same bounded cache.
use super::*;
use crate::{Spec, source::Resolve};

/// One square of a style to paint, with an opaque ticket and cache lifetime.
#[derive(Clone)]
pub struct Request {
    pub id: u64,
    pub epoch: u64,
    pub style: u64,
    pub spec: Arc<Spec>,
    pub frame: Frame,
    pub side: u32,
}

/// `Ok(None)` discards obsolete work so it can be requested again if needed. An error uses
/// the synchronous painter for that square; it never leaves an unanswerable pending job.
pub type Done = Box<dyn FnOnce(Result<Option<Pixmap>, String>) + Send>;

/// The host owns scheduling and transport, including cancellation of obsolete queued jobs.
pub trait Painter: Send + Sync {
    fn paint(&self, request: Request, done: Done);
    fn cancel(&self, id: u64);
}

fn host() -> &'static Mutex<Option<Arc<dyn Painter>>> {
    static HOST: OnceLock<Mutex<Option<Arc<dyn Painter>>>> = OnceLock::new();
    HOST.get_or_init(Default::default)
}

/// Installs a background painter, or returns to synchronous drawing. Outstanding answers
/// from the previous painter cannot overwrite pictures built by the new one.
pub fn set_painter(painter: Option<Arc<dyn Painter>>) {
    let old = std::mem::replace(&mut *host().lock().unwrap_or_else(|e| e.into_inner()), painter);
    let ids: Vec<u64> = lock().pending.drain().map(|(_, (id, _))| id).collect();
    if let Some(old) = old {
        for id in ids {
            old.cancel(id);
        }
    }
    cache::notify();
}

/// Paints one request in the worker. `None` means its style, tiles or sprite are still loading;
/// retry after the worker's tile cache wakes. Labels are collected separately by the UI.
pub fn draw(spec: &Spec, frame: &Frame, side: u32) -> Result<Option<Pixmap>, String> {
    if side == 0
        || side > MAX_SIDE as u32
        || !frame.m_per_px.is_finite()
        || !(1e-9..=1e10).contains(&frame.m_per_px)
        || !frame.ppp.is_finite()
        || !(0.01..=64.0).contains(&frame.ppp)
        || !frame.left.is_finite()
        || !frame.top.is_finite()
    {
        return Err("invalid background tile dimensions".into());
    }
    let r = match crate::source::resolve(spec) {
        Resolve::Ready(r) => r,
        Resolve::Pending => return Ok(None),
        Resolve::Failed(e) => return Err(e),
    };
    let mut pm = Pixmap::new(side, side).ok_or("could not allocate background tile")?;
    let e = frame.extent(side, side);
    let zoom = template::style_zoom(frame.m_per_px * f64::from(frame.ppp));
    let reach = r.style.layers.iter().filter(|l| l.visible_at(zoom)).map(|l| overhang(l, zoom)).fold(0.0f32, f32::max);
    let pad = f64::from(reach * frame.ppp) * frame.m_per_px;
    let view = Extent::new(e.min_x - pad, e.min_y - pad, e.max_x + pad, e.max_y + pad);
    let mut out = Drawn::default();
    draw_layers(Some(&mut pm), (side, side), &view, &r, frame, Pass::Shapes, &mut out);
    Ok((!out.pending).then_some(pm))
}

/// Validates a worker's square RGBA answer before taking ownership of its pixels.
pub fn from_pixels(side: u32, bytes: Vec<u8>) -> Option<Pixmap> {
    if side > MAX_SIDE as u32 {
        return None;
    }
    Pixmap::from_vec(bytes, tiny_skia::IntSize::from_wh(side, side)?)
}

const MAX_PENDING: usize = 64;

#[allow(clippy::too_many_arguments)]
pub(super) fn enqueue(
    spec: &Spec,
    r: &Arc<Resolved>,
    todo: &[Placed],
    key: &impl Fn(TileId) -> Key,
    side: u32,
    m_per_px: f64,
    ppp: f32,
    pending: &mut bool,
) -> bool {
    let Some(host) = host().lock().unwrap_or_else(|e| e.into_inner()).clone() else { return false };
    if todo.is_empty() {
        return true;
    }
    // An unsupported host/format falls back for this style/scale without repeated failures.
    if lock().failed.contains(&(Arc::as_ptr(r) as usize)) {
        return false;
    }
    let spec = Arc::new(spec.clone());
    let wanted: HashSet<Key> = todo.iter().map(|p| key(p.id)).collect();
    let mut order = todo.to_vec();
    let center = order
        .iter()
        .map(|p| {
            let e = p.extent();
            ((e.min_x + e.max_x) * 0.5, (e.min_y + e.max_y) * 0.5)
        })
        .fold((0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
    let n = order.len().max(1) as f64;
    let distance = |p: &Placed| {
        let e = p.extent();
        ((e.min_x + e.max_x) * 0.5 - center.0 / n).powi(2) + ((e.min_y + e.max_y) * 0.5 - center.1 / n).powi(2)
    };
    // Nearest first; the host processes the newest requests first too.
    order.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
    let mut cancelled = Vec::new();
    let mut requests = Vec::new();
    {
        let mut s = lock();
        for p in &order {
            let k = key(p.id);
            let used = s.tick;
            if let Some((_, touched)) = s.pending.get_mut(&k) {
                *touched = used;
                continue;
            }
            if s.pending.len() >= MAX_PENDING {
                let old =
                    s.pending.iter().filter(|(k, _)| !wanted.contains(k)).min_by_key(|(_, (id, _))| *id).map(|(k, (id, _))| (*k, *id));
                let Some((old, id)) = old else {
                    continue;
                };
                s.pending.remove(&old);
                cancelled.push(id);
            }
            s.ticket = s.ticket.wrapping_add(1);
            let id = s.ticket;
            s.pending.insert(k, (id, used));
            let e = p.extent();
            requests.push((
                k,
                Request {
                    id,
                    epoch: s.epoch,
                    style: k.style as u64,
                    spec: spec.clone(),
                    frame: Frame { left: e.min_x, top: e.max_y, m_per_px, ppp },
                    side,
                },
            ));
        }
    }
    for id in cancelled {
        host.cancel(id);
    }
    for (key, req) in requests.into_iter().rev() {
        let id = req.id;
        let weak = Arc::downgrade(r);
        host.paint(
            req,
            Box::new(move |answer| {
                {
                    let mut s = lock();
                    if s.pending.get(&key).map(|(ticket, _)| *ticket) != Some(id) {
                        return;
                    }
                    let used = s.pending.remove(&key).map_or(s.tick, |(_, used)| used);
                    if weak.strong_count() == 0 {
                        return;
                    }
                    match answer {
                        Ok(Some(pm)) if pm.width() == side && pm.height() == side => {
                            let bytes = pm.data().len();
                            let entry = Entry { style: weak, picture: Arc::new(Payload::Image(pm)), bytes, used, waiting: None };
                            s.bytes += bytes;
                            if let Some(old) = s.entries.insert(key, entry) {
                                s.bytes = s.bytes.saturating_sub(old.bytes);
                            }
                            evict(&mut s);
                        }
                        Ok(Some(_)) | Err(_) => {
                            if s.failed.len() >= 2048 {
                                s.failed.clear();
                            }
                            s.failed.insert(key.style);
                        }
                        Ok(None) => {}
                    }
                }
                cache::notify();
            }),
        );
    }
    *pending = true;
    true
}

/// Keep the nearest available zoom underneath pending squares, including at integer zoom
/// boundaries. Each square uses one zoom only, so translucent styles aren't drawn twice.
pub(super) fn fallback(s: &Store, r: &Arc<Resolved>, wanted: &Key, p: &Placed) -> Vec<Item> {
    let wrap = p.wx.div_euclid(1_i64 << p.id.z.min(30));
    let at = |k: &Key| Placed { id: k.tile, wx: i64::from(k.tile.x) + wrap * (1_i64 << k.tile.z.min(30)) }.extent();
    let eligible = |k: &&Key, e: &&Entry| {
        k.style == wanted.style
            && (k.step - wanted.step).abs() <= STEPS as i32
            && e.style.upgrade().is_some_and(|style| Arc::ptr_eq(&style, r))
            && at(k).intersects(&p.extent())
    };
    let quality = |k: &Key| ((f32::from_bits(k.ppp) / f32::from_bits(wanted.ppp)).log2().abs()).to_bits();
    let choice = s
        .entries
        .iter()
        .filter(|(k, e)| eligible(k, e))
        .min_by_key(|(k, _)| ((k.step - wanted.step).abs(), quality(k)))
        .map(|(k, _)| (k.step, k.ppp));
    s.entries
        .iter()
        .filter(|(k, e)| Some((k.step, k.ppp)) == choice && eligible(k, e))
        .map(|(k, e)| {
            let at = at(k);
            let cell = p.extent();
            let clip = Extent::new(at.min_x.max(cell.min_x), at.min_y.max(cell.min_y), at.max_x.min(cell.max_x), at.max_y.min(cell.max_y));
            Item { payload: e.picture.clone(), at, clip, z: k.tile.z }
        })
        .collect()
}
