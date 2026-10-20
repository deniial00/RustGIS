//! Vector tile pictures are painted in a separate WASM instance. Only pixels cross back to
//! the UI; no shared memory, cross-origin isolation or worker-side GPU context is required.
use rustgis_engine::render::rustgis_tiles::{
    self as tiles, Frame, Spec,
    picture::worker::{self, Done, Painter, Request},
};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};

struct Host {
    worker: web_sys::Worker,
    ready: bool,
    queued: Vec<u64>,
    epoch: Option<u64>,
    pending: HashMap<u64, (Request, Done)>,
    styles: HashMap<u64, Arc<Spec>>,
    url: Option<String>,
}
thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
    static DRAWER: RefCell<Option<Drawer>> = const { RefCell::new(None) };
}
struct Bridge;
fn error(e: JsValue) -> String {
    format!("{e:?}")
}
fn post(worker: &web_sys::Worker, value: Value) -> Result<(), String> {
    worker.post_message(&JsValue::from_str(&value.to_string())).map_err(error)
}
fn send(host: &mut Host, req: &Request) -> Result<(), String> {
    if host.styles.get(&req.style).is_none_or(|s| **s != *req.spec) {
        post(&host.worker, json!({"kind":"style", "style":req.style, "spec":&*req.spec}))?;
        if host.styles.len() >= 256 {
            host.styles.clear();
        }
        host.styles.insert(req.style, req.spec.clone());
    }
    post(&host.worker, json!({"kind":"paint", "id":req.id, "style":req.style, "frame":req.frame, "side":req.side}))
}
impl Painter for Bridge {
    fn paint(&self, req: Request, done: Done) {
        let mut old = Vec::new();
        let failed = HOST.with(|cell| {
            let mut h = cell.borrow_mut();
            let Some(h) = h.as_mut() else {
                old.push(done);
                return true;
            };
            if h.epoch != Some(req.epoch) {
                old.extend(h.pending.drain().map(|(_, (_, d))| d));
                h.styles.clear();
                h.queued.clear();
                h.epoch = Some(req.epoch);
                if h.ready && post(&h.worker, json!({"kind":"reset"})).is_err() {
                    old.push(done);
                    return true;
                }
            }
            let result = if h.ready { send(h, &req) } else { Ok(()) };
            if !h.ready {
                h.queued.push(req.id);
            }
            h.pending.insert(req.id, (req, done));
            result.is_err()
        });
        for done in old {
            done(Ok(None));
        }
        if failed {
            fail_host();
        }
    }
    fn cancel(&self, id: u64) {
        let done = HOST.with(|cell| {
            let mut h = cell.borrow_mut();
            let h = h.as_mut()?;
            if h.ready {
                let _ = post(&h.worker, json!({"kind":"cancel", "id":id}));
            }
            h.queued.retain(|&ticket| ticket != id);
            h.pending.remove(&id).map(|(_, done)| done)
        });
        if let Some(done) = done {
            done(Ok(None));
        }
    }
}
fn fail_host() {
    let host = HOST.with(|h| h.borrow_mut().take());
    worker::set_painter(None);
    if let Some(mut host) = host {
        host.worker.terminate();
        if let Some(url) = host.url.take() {
            let _ = web_sys::Url::revoke_object_url(&url);
        }
        for (_, (_, done)) in host.pending {
            done(Err("background tile worker unavailable".into()));
        }
    }
}

/// Starts one worker using the already-compiled module and Trunk's module preload link.
/// A failed/unsupported worker falls back to the regular renderer.
pub fn install() -> Result<(), String> {
    let document = web_sys::window().and_then(|w| w.document()).ok_or("no document")?;
    let link = document
        .query_selector("link[rel='modulepreload'][href$='.js']:not([href*='/snippets/'])")
        .map_err(error)?
        .and_then(|el| el.get_attribute("href"))
        .ok_or("no WASM module preload")?;
    let base = document.base_uri().map_err(error)?.ok_or("no document base")?;
    let module = web_sys::Url::new_with_base(&link, &base).map_err(error)?.href();
    let module = serde_json::to_string(&module).map_err(|e| e.to_string())?;
    let script = format!(
        r#"import init from {module};
self.onmessage=async e=>{{self.onmessage=null;try{{await init({{module_or_path:e.data}})}}catch(_){{self.postMessage({{kind:'failed'}})}}}};"#
    );
    let parts = js_sys::Array::new();
    parts.push(&JsValue::from_str(&script));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("text/javascript");
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &opts).map_err(error)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(error)?;
    let options = web_sys::WorkerOptions::new();
    options.set_type(web_sys::WorkerType::Module);
    let worker = match web_sys::Worker::new_with_options(&url, &options) {
        Ok(worker) => worker,
        Err(e) => {
            let _ = web_sys::Url::revoke_object_url(&url);
            return Err(error(e));
        }
    };
    let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |e: web_sys::MessageEvent| receive(e.data()));
    worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    on_message.forget();
    let on_error = Closure::<dyn FnMut(web_sys::ErrorEvent)>::new(move |_| fail_host());
    worker.set_onerror(Some(on_error.as_ref().unchecked_ref()));
    on_error.forget();
    HOST.with(|h| {
        *h.borrow_mut() = Some(Host {
            worker: worker.clone(),
            ready: false,
            queued: Vec::new(),
            epoch: None,
            pending: HashMap::new(),
            styles: HashMap::new(),
            url: Some(url),
        })
    });
    if let Err(e) = worker.post_message(&wasm_bindgen::module()) {
        fail_host();
        return Err(error(e));
    }
    worker::set_painter(Some(Arc::new(Bridge)));
    Ok(())
}
fn field(v: &JsValue, key: &str) -> Option<JsValue> {
    js_sys::Reflect::get(v, &JsValue::from_str(key)).ok()
}
fn receive(data: JsValue) {
    let kind = field(&data, "kind").and_then(|x| x.as_string()).unwrap_or_default();
    if kind == "ready" {
        let failed = HOST.with(|h| {
            let mut h = h.borrow_mut();
            let Some(h) = h.as_mut() else { return false };
            h.ready = true;
            if let Some(url) = h.url.take() {
                let _ = web_sys::Url::revoke_object_url(&url);
            }
            if post(&h.worker, json!({"kind":"reset"})).is_err() {
                return true;
            }
            let requests: Vec<Request> =
                std::mem::take(&mut h.queued).into_iter().filter_map(|id| h.pending.get(&id).map(|(r, _)| r.clone())).collect();
            requests.iter().any(|r| send(h, r).is_err())
        });
        if failed {
            fail_host();
        }
        return;
    }
    if kind == "failed" {
        fail_host();
        return;
    }
    let Some(id) = field(&data, "id").and_then(|x| x.as_string()).and_then(|x| x.parse::<u64>().ok()) else { return };
    let pending = HOST.with(|h| h.borrow_mut().as_mut().and_then(|h| h.pending.remove(&id)));
    let Some((req, done)) = pending else { return };
    let answer = if kind == "discarded" {
        Ok(None)
    } else if kind == "painted" {
        let pixels = field(&data, "pixels").and_then(|x| x.dyn_into::<js_sys::Uint8Array>().ok());
        let expected = req.side.checked_mul(req.side).and_then(|n| n.checked_mul(4));
        pixels
            .filter(|p| Some(p.length()) == expected)
            .and_then(|p| worker::from_pixels(req.side, p.to_vec()))
            .map(Some)
            .ok_or_else(|| "invalid worker pixels".into())
    } else {
        Err("background tile painting failed".into())
    };
    done(answer);
}

struct Job {
    id: u64,
    spec: Arc<Spec>,
    frame: Frame,
    side: u32,
    tried: Option<u64>,
}
struct Drawer {
    scope: web_sys::DedicatedWorkerGlobalScope,
    styles: HashMap<u64, Arc<Spec>>,
    jobs: Vec<Job>,
    scheduled: bool,
}
fn respond(scope: &web_sys::DedicatedWorkerGlobalScope, kind: &str, id: Option<u64>, pixels: Option<Vec<u8>>) {
    let msg = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&msg, &"kind".into(), &kind.into());
    if let Some(id) = id {
        let _ = js_sys::Reflect::set(&msg, &"id".into(), &id.to_string().into());
    }
    if let Some(pixels) = pixels {
        let array = js_sys::Uint8Array::from(pixels.as_slice());
        let _ = js_sys::Reflect::set(&msg, &"pixels".into(), &array);
        let transfers = js_sys::Array::new();
        transfers.push(&array.buffer());
        if scope.post_message_with_transfer(&msg, &transfers).is_ok() {
            return;
        }
        respond(scope, "error", id, None);
    } else {
        let _ = scope.post_message(&msg);
    }
}
/// Worker entry point; it never starts egui or initializes a GPU device.
pub fn run() -> Result<(), String> {
    let scope = js_sys::global().dyn_into::<web_sys::DedicatedWorkerGlobalScope>().map_err(|e| error(e.into()))?;
    tiles::cache::set_fetcher(|url, done| wasm_bindgen_futures::spawn_local(async move { done(crate::web::fetch_bytes(&url).await) }));
    tiles::cache::set_waker(schedule);
    DRAWER.with(|d| *d.borrow_mut() = Some(Drawer { scope: scope.clone(), styles: HashMap::new(), jobs: Vec::new(), scheduled: false }));
    let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
        let Some(v) = event.data().as_string().and_then(|s| serde_json::from_str::<Value>(&s).ok()) else { return };
        let Some(kind) = v.get("kind").and_then(Value::as_str) else { return };
        DRAWER.with(|d| {
            let mut d = d.borrow_mut();
            let Some(d) = d.as_mut() else { return };
            match kind {
                "reset" => {
                    d.jobs.clear();
                    d.styles.clear();
                    tiles::cache::clear();
                }
                "cancel" => {
                    if let Some(id) = v.get("id").and_then(Value::as_u64) {
                        d.jobs.retain(|j| j.id != id);
                    }
                }
                "style" => {
                    if let (Some(id), Some(spec)) = (
                        v.get("style").and_then(Value::as_u64),
                        v.get("spec").cloned().and_then(|s| serde_json::from_value::<Spec>(s).ok()),
                    ) {
                        if d.styles.len() >= 256 {
                            d.styles.clear();
                        }
                        d.styles.insert(id, Arc::new(spec));
                    }
                }
                "paint" => {
                    let id = v.get("id").and_then(Value::as_u64);
                    let job = v
                        .get("id")
                        .and_then(Value::as_u64)
                        .zip(v.get("style").and_then(Value::as_u64))
                        .zip(v.get("side").and_then(Value::as_u64))
                        .zip(v.get("frame").cloned().and_then(|f| serde_json::from_value::<Frame>(f).ok()));
                    if let Some((((id, style), side), frame)) = job {
                        if let Some(spec) = d.styles.get(&style).cloned().filter(|_| side > 0 && side <= 2048) {
                            if d.jobs.len() >= 64 {
                                let old = d.jobs.remove(0);
                                respond(&d.scope, "discarded", Some(old.id), None);
                            }
                            d.jobs.push(Job { id, spec, frame, side: side as u32, tried: None });
                        } else {
                            respond(&d.scope, "error", Some(id), None);
                        }
                    } else if let Some(id) = id {
                        respond(&d.scope, "error", Some(id), None);
                    }
                }
                _ => {}
            }
        });
        schedule();
    });
    scope.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    on_message.forget();
    respond(&scope, "ready", None, None);
    Ok(())
}
fn schedule() {
    let scope = DRAWER.with(|d| {
        let mut d = d.borrow_mut();
        let d = d.as_mut()?;
        if d.scheduled || !d.jobs.iter().any(|j| j.tried != Some(tiles::cache::generation())) {
            return None;
        }
        d.scheduled = true;
        Some(d.scope.clone())
    });
    if let Some(scope) = scope {
        let callback = Closure::once_into_js(process_one);
        let _ = scope.set_timeout_with_callback_and_timeout_and_arguments_0(callback.unchecked_ref(), 0);
    }
}
fn process_one() {
    let next = DRAWER.with(|d| {
        let mut d = d.borrow_mut();
        let d = d.as_mut()?;
        d.scheduled = false;
        let generation = tiles::cache::generation();
        let i = d.jobs.iter().rposition(|j| j.tried != Some(generation))?;
        Some((d.jobs.remove(i), d.scope.clone(), generation))
    });
    if let Some((mut job, scope, generation)) = next {
        match worker::draw(&job.spec, &job.frame, job.side) {
            Ok(Some(pm)) => respond(&scope, "painted", Some(job.id), Some(pm.take())),
            Ok(None) => {
                job.tried = Some(generation);
                DRAWER.with(|d| {
                    if let Some(d) = d.borrow_mut().as_mut() {
                        d.jobs.push(job);
                    }
                });
            }
            Err(_) => respond(&scope, "error", Some(job.id), None),
        }
    }
    schedule();
}
