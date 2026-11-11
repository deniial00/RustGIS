//! Changed on disk (#1477): the desktop watches the files its layers and local tile archives read,
//! every few seconds and when the window regains focus, and offers Reload Data for a source that
//! changed. Only metadata is read (`rustgis_engine::observe`), on a thread of its own; reloading is
//! always the user's choice and always goes through `ui.reloadData` or `map.reloadTiles`.
//!
//! [`Monitor`] is the state machine, driven by observations and nothing else; [`Watch`] finds the
//! targets, runs the scans and shows what the monitor says. Monitor state is the app's alone: it
//! never dirties the project or adds an undo step.
use std::collections::{HashMap, HashSet};

use rustgis_engine::Session;
use rustgis_engine::observe::Observation;
use serde_json::{Value, json};

use crate::RustGisApp;
use crate::theme::Tokens;

/// How often the files are looked at while the app is open.
const EVERY: web_time::Duration = web_time::Duration::from_secs(5);
/// How long a scan may go unanswered before the user is told checks are waiting.
const STALLED: web_time::Duration = web_time::Duration::from_secs(30);

/// What one scan found at a source's linked path.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Seen {
    Present(Observation),
    Missing,
    /// Its metadata couldn't be read (permissions, a locked or odd file).
    Unreadable(String),
    /// A folder (a Zarr store, a File Geodatabase): not monitored, its timestamp says nothing of
    /// the files inside. Reload Data reads it on request.
    Folder,
}

fn look(path: &str) -> Seen {
    if std::path::Path::new(path).is_dir() {
        return Seen::Folder;
    }
    match Observation::source(path) {
        Ok(o) if o.missing() => Seen::Missing,
        Ok(o) => Seen::Present(o),
        Err(e) => Seen::Unreadable(e),
    }
}

/// A reload target: a layer, or a local archive, in one map of the open project. Layer ids are
/// map-local and reused, so a layer is known by its instance and its map's; an archive by its
/// path and which of the map's archives with that path it is (`nth`, from the top).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Target {
    Layer { map: u64, layer: u64 },
    Tiles { map: u64, url: String, nth: usize },
}

/// What the app shows for a target.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Status {
    Unchanged,
    Changed,
    Missing,
    Unreadable(String),
    /// A folder source: Reload Data only, said in the layer's details.
    Folder,
}

#[derive(Default)]
struct Source {
    /// The latest scan and its number.
    last: Option<(Seen, u64)>,
    /// What two consecutive scans agreed on: a write still under way isn't offered.
    settled: Option<Seen>,
    /// RustGIS wrote the file itself: what was there just before, the number of the first scan
    /// that started after the write, and what was there just after it (RustGIS's result).
    wrote: Option<(Seen, u64, Seen)>,
}

struct Tracked {
    /// The coalescing key of the linked path; a repair changes it, and the target starts over.
    key: String,
    /// The observation the target's data was read with, as far as the app knows.
    baseline: Option<Seen>,
    /// Not Now: the observation it was given for. A newer one notifies again.
    dismissed: Option<Seen>,
}

/// Change detection for every target, by observations alone (no clock, no files).
#[derive(Default)]
pub(crate) struct Monitor {
    sources: HashMap<String, Source>,
    targets: HashMap<Target, Tracked>,
    /// Each reload read under way and what its file held as it started.
    reading: HashMap<Target, Seen>,
}

impl Monitor {
    /// Follows the project's targets now (each with its source's key): new ones start with no
    /// baseline, gone ones are forgotten, one whose path changed starts over. `true` when a
    /// target has nothing to compare with yet, so a scan is wanted now.
    pub(crate) fn track(&mut self, found: impl IntoIterator<Item = (Target, String)>) -> bool {
        let mut seen = HashSet::new();
        for (target, key) in found {
            match self.targets.get_mut(&target) {
                Some(t) if t.key == key => {}
                _ => {
                    self.targets.insert(target.clone(), Tracked { key: key.clone(), baseline: None, dismissed: None });
                }
            }
            seen.insert(target);
        }
        self.targets.retain(|target, _| seen.contains(target));
        let keys: HashSet<&String> = self.targets.values().map(|t| &t.key).collect();
        self.sources.retain(|key, _| keys.contains(key));
        self.targets.values().any(|t| t.baseline.is_none())
    }

    /// Whether a target reads `key`.
    pub(crate) fn watches(&self, key: &str) -> bool {
        self.targets.values().any(|t| t.key == key)
    }

    /// The keys to scan.
    #[cfg(test)]
    pub(crate) fn keys(&self) -> HashSet<String> {
        self.targets.values().map(|t| t.key.clone()).collect()
    }

    /// Gives a target the observation its data was read with.
    #[cfg(test)]
    pub(crate) fn baseline(&mut self, target: &Target, seen: Seen) {
        if let Some(t) = self.targets.get_mut(target) {
            t.baseline = Some(seen);
        }
    }

    /// Scan number `scan` saw `seen` at `key`.
    pub(crate) fn observed(&mut self, key: &str, seen: Seen, scan: u64) {
        if !self.targets.values().any(|t| t.key == key) {
            return;
        }
        let source = self.sources.entry(key.to_string()).or_default();
        let agreed = source.last.as_ref().is_some_and(|(last, _)| *last == seen);
        let since = source.last.as_ref().map(|(_, n)| *n);
        source.last = Some((seen.clone(), scan));
        if agreed {
            source.settled = Some(seen.clone());
        }
        // Self-write: acknowledged once two scans, both started after it, agree.
        let acknowledged = match &source.wrote {
            Some((before, after, result)) if agreed && since.is_some_and(|n| n >= *after) => Some((before.clone(), result.clone())),
            _ => None,
        };
        if acknowledged.is_some() {
            source.wrote = None;
        }
        for t in self.targets.values_mut().filter(|t| t.key == key) {
            // A target added since: the first scan is what it was read with.
            if t.baseline.is_none() {
                t.baseline = Some(seen.clone());
            }
            if let Some((before, result)) = &acknowledged
                && t.baseline.as_ref() == Some(before)
            {
                t.baseline = Some(result.clone());
            }
        }
    }

    fn settled(&self, t: &Tracked) -> Option<&Seen> {
        self.sources.get(&t.key).and_then(|s| s.settled.as_ref())
    }

    pub(crate) fn status(&self, target: &Target) -> Status {
        let Some(t) = self.targets.get(target) else { return Status::Unchanged };
        match self.settled(t) {
            Some(Seen::Missing) => Status::Missing,
            Some(Seen::Unreadable(e)) => Status::Unreadable(e.clone()),
            Some(Seen::Folder) => Status::Folder,
            Some(seen @ Seen::Present(_)) if t.baseline.as_ref().is_some_and(|b| b != seen) => Status::Changed,
            _ => Status::Unchanged,
        }
    }

    /// Whether the notification lists the target: it isn't up to date and Not Now wasn't
    /// chosen for what is there now.
    pub(crate) fn notifies(&self, target: &Target) -> bool {
        let Some(t) = self.targets.get(target) else { return false };
        !matches!(self.status(target), Status::Unchanged | Status::Folder) && t.dismissed.as_ref() != self.settled(t)
    }

    /// Not Now: the target stays marked, the notification leaves it until something newer.
    pub(crate) fn dismiss(&mut self, target: &Target) {
        let settled = self.targets.get(target).and_then(|t| self.settled(t)).cloned();
        if let Some(t) = self.targets.get_mut(target) {
            t.dismissed = settled;
        }
    }

    /// A reload's read starts, its file holding `seen` (looked at just now).
    pub(crate) fn reading(&mut self, target: Target, seen: Seen) {
        self.reading.insert(target, seen);
    }

    /// A read started for `target` ended without a result (it failed, or never ran).
    pub(crate) fn not_read(&mut self, target: &Target) {
        self.reading.remove(target);
    }

    /// A reload succeeded: what its file held as its read started ([`Monitor::reading`]) is
    /// acknowledged, so a change that came while it read stays pending. A reload with no such
    /// observation (run some other way) leaves the mark as it is: what it read isn't known.
    pub(crate) fn reloaded(&mut self, target: &Target) {
        let used = self.reading.remove(target);
        if let Some(t) = self.targets.get_mut(target)
            && matches!(used, Some(Seen::Present(_)))
        {
            t.baseline = used;
            t.dismissed = None;
        }
    }

    /// RustGIS wrote `key` (a GeoPackage save, an export into it); `before` is what was there
    /// just before the write and `next_scan` the number the next scan will have. Only targets
    /// whose data matched `before` take `result` (what was there just after the write) as
    /// theirs, once two scans after the write agree; a change another program makes before or
    /// after the save then still differs from what they hold.
    pub(crate) fn wrote(&mut self, key: &str, before: Seen, result: Seen, next_scan: u64) {
        if !self.watches(key) {
            return;
        }
        let source = self.sources.entry(key.to_string()).or_default();
        // A second write right after the first, with nothing between them, continues it.
        let before = match source.wrote.take() {
            Some((first_before, _, first_result)) if first_result == before => first_before,
            _ => before,
        };
        source.wrote = Some((before, next_scan, result));
    }
}

/// A path as the targets reading it are grouped: absolute and lexically normal, without
/// resolving links (the linked path itself is what is watched).
pub(crate) fn key(path: &str) -> String {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.into());
    let mut out = std::path::PathBuf::new();
    for part in absolute.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out.display().to_string()
}

/// One target as the project has it now.
struct Found {
    target: Target,
    key: String,
    path: String,
    map: String,
    name: String,
    /// Repair Data Source can relink it (an editable GeoPackage layer without a linked file can't).
    repairable: bool,
}

/// The linked local sources in every map: layers Reload Data reads and local archives. Remote,
/// embedded and in-memory data aren't targets; a converted (GDAL, DuckDB) layer is watched at the
/// original file, not the conversion.
fn targets(s: &Session) -> Vec<Found> {
    let mut out = Vec::new();
    for m in &s.project.maps {
        for l in &m.layers {
            if let Some((path, _, _)) = crate::reloaddata::source(l) {
                let target = Target::Layer { map: m.instance_id(), layer: l.instance_id() };
                let repairable = l.linked_file().is_some();
                out.push(Found { target, key: key(path), path: path.to_string(), map: m.name.clone(), name: l.name.clone(), repairable });
            }
        }
        let mut archives: HashMap<&str, usize> = HashMap::new();
        for tile in &m.tiles {
            if let Some(url) = tile.archive_file() {
                let nth = archives.entry(url).or_default();
                let path = url.strip_prefix("file://").unwrap_or(url);
                let target = Target::Tiles { map: m.instance_id(), url: url.to_string(), nth: *nth };
                *nth += 1;
                out.push(Found {
                    target,
                    key: key(path),
                    path: path.to_string(),
                    map: m.name.clone(),
                    name: tile.name.clone(),
                    repairable: true,
                });
            }
        }
    }
    out
}

type Scanned = crate::Slot<Vec<(String, Seen)>>;

/// The app's side of the monitor: its targets, its one scan at a time and when the next is due.
#[derive(Default)]
pub struct Watch {
    monitor: Monitor,
    found: Vec<Found>,
    opened: u64,
    revision: Option<u64>,
    scans: u64,
    scan: Option<(u64, Scanned, web_time::Instant)>,
    due: Option<web_time::Instant>,
    wanted: bool,
    focused: bool,
    /// A scan couldn't start, or one hasn't answered for [`STALLED`]: said once each.
    said_failed: bool,
    said_stalled: bool,
}

impl Watch {
    fn status(&self, target: &Target) -> Status {
        self.monitor.status(target)
    }
}

/// What the Layers pane says of a layer of the active map.
pub(crate) fn layer_status(app: &RustGisApp, layer: &rustgis_engine::map::Layer) -> Status {
    app.diskwatch.status(&Target::Layer { map: app.session.map().instance_id(), layer: layer.instance_id() })
}

/// The target of tile layer `i` of `map`: its archive and which of the map's archives with that
/// path it is.
fn tiles_target(map: &rustgis_engine::map::Map, i: usize) -> Option<Target> {
    let url = map.tiles.get(i)?.archive_file()?;
    let nth = map.tiles.iter().take(i).filter(|t| t.archive_file() == Some(url)).count();
    Some(Target::Tiles { map: map.instance_id(), url: url.to_string(), nth })
}

/// What the Layers pane says of tile layer `i` of the active map.
pub(crate) fn tiles_status(app: &RustGisApp, i: usize) -> Status {
    match tiles_target(app.session.map(), i) {
        Some(target) => app.diskwatch.status(&target),
        None => Status::Unchanged,
    }
}

/// The Layers pane's words for a status: a note after the name (`""` for none), a line for its
/// details, and how the name is tinted: `Some(true)` red, `Some(false)` amber, `None` as it is.
pub(crate) fn row_note(status: &Status) -> Option<(String, String, Option<bool>)> {
    match status {
        Status::Unchanged => None,
        Status::Changed => Some((
            tl!("(changed on disk)").to_string(),
            tl!("Changed on disk: the file may have changed since it was read. Reload Data reads it again.").to_string(),
            Some(false),
        )),
        Status::Missing => Some((
            tl!("(missing on disk)").to_string(),
            tl!("The file is missing. Restore it, or use Repair Data Source.").to_string(),
            Some(true),
        )),
        Status::Unreadable(e) => Some((
            tl!("(can't be checked)").to_string(),
            crate::i18n::fmt("The file can't be checked: {error}", &[("error", e)]),
            Some(true),
        )),
        Status::Folder => Some((String::new(), tl!("Folder source — changes are not monitored. Use Reload Data.").to_string(), None)),
    }
}

/// Each frame: follows the project's targets, lands a finished scan and starts one when due.
pub(crate) fn tick(app: &mut RustGisApp, ctx: &egui::Context) {
    // A file an export under way writes into is the export's work until it lands.
    let exporting = crate::exportjob::writing(app).map(String::from);
    let w = &mut app.diskwatch;
    if w.opened != app.session.opened {
        // Another project: nothing of the old one's scans applies.
        *w = Watch { opened: app.session.opened, focused: w.focused, ..Watch::default() };
    }
    if w.revision != Some(app.session.revision) {
        w.revision = Some(app.session.revision);
        w.found = targets(&app.session);
        w.wanted |= w.monitor.track(w.found.iter().map(|f| (f.target.clone(), f.key.clone())));
    }
    let focused = ctx.input(|i| i.focused);
    w.wanted |= focused && !w.focused;
    w.focused = focused;
    if let Some((scan, slot, _)) = &w.scan {
        let done = slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        if let Some(results) = done {
            let scan = *scan;
            w.scan = None;
            for (key, seen) in results {
                if exporting.as_deref() != Some(key.as_str()) {
                    w.monitor.observed(&key, seen, scan);
                }
            }
        }
    }
    let now = web_time::Instant::now();
    // A file that never answers (a network drive gone away) holds the scan, and with it every
    // later one: said, once, rather than shown as up to date.
    let mut say = None;
    if !w.said_stalled && w.scan.as_ref().is_some_and(|(_, _, started)| now.saturating_duration_since(*started) >= STALLED) {
        w.said_stalled = true;
        say = Some(tl!("Checking linked files for changes on disk is waiting for a file that doesn't answer (a network drive?). Changes aren't reported until it does, and reloading or saving a file there may wait for it too.").to_string());
    }
    if !w.found.is_empty() {
        if w.due.is_none_or(|due| now >= due) {
            w.wanted = true;
        }
        if w.wanted
            && w.scan.is_none()
            && let Err(e) = start(w, now)
        {
            log::warn!("changed-on-disk scan could not start: {e}");
            if !w.said_failed {
                w.said_failed = true;
                say = Some(crate::i18n::fmt(
                    "Checking linked files for changes on disk couldn't start ({error}); it is tried again every few seconds.",
                    &[("error", &e)],
                ));
            }
        }
        let wait = if w.scan.is_some() {
            web_time::Duration::from_millis(100)
        } else {
            w.due.map_or(EVERY, |due| due.saturating_duration_since(now))
        };
        ctx.request_repaint_after(wait);
    }
    if let Some(text) = say {
        app.session.notify(text, true);
    }
}

/// Starts a scan of every watched path on a thread of its own; why not, when the OS refuses one
/// (that costs this scan, not the app: the next is tried when due).
fn start(w: &mut Watch, now: web_time::Instant) -> Result<(), String> {
    // The linked path is observed as given (one per key), never a resolved copy of it.
    let mut paths: HashMap<String, String> = HashMap::new();
    for f in &w.found {
        paths.entry(f.key.clone()).or_insert_with(|| f.path.clone());
    }
    let slot = Scanned::default();
    let out = slot.clone();
    let spawned = std::thread::Builder::new().name("Changed on disk".into()).spawn(move || {
        let results = paths.into_iter().map(|(key, path)| (key, look(&path))).collect();
        *out.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(results);
    });
    w.due = Some(now + EVERY);
    w.wanted = false;
    spawned.map_err(|e| e.to_string())?;
    w.scans += 1;
    w.scan = Some((w.scans, slot, now));
    Ok(())
}

/// The target a reload command names in the active map, if it's watched.
fn target_of(app: &RustGisApp, command: &str, params: &Value, answer: Option<&Value>) -> Option<Target> {
    let map = app.session.map();
    match command {
        "layer.reloadData" | "gpkg.discardEdits" => {
            let id = answer.and_then(|a| a.get("layer")).and_then(Value::as_u64).and_then(|id| u32::try_from(id).ok());
            let id = id.or_else(|| app.session.layer_id(params).ok())?;
            Some(Target::Layer { map: map.instance_id(), layer: map.layer(id)?.instance_id() })
        }
        "map.reloadTiles" => {
            let i = match answer.and_then(|a| a.get("index")).and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok()) {
                Some(i) => i,
                None => rustgis_engine::cmd::reads::archive_target(&app.session, params)?.0,
            };
            tiles_target(map, i)
        }
        _ => None,
    }
}

/// A reload's read starts (on its thread, or here): what its file holds now is what it reads,
/// and what it acknowledges once it lands (one metadata read of one file).
pub(crate) fn reading(app: &mut RustGisApp, command: &str, params: &Value) {
    if !app.watch_files {
        return;
    }
    let Some(target) = target_of(app, command, params, None) else { return };
    let Some(path) = app.diskwatch.found.iter().find(|f| f.target == target).map(|f| f.path.clone()) else { return };
    app.diskwatch.monitor.reading(target, look(&path));
}

/// A reload's read failed or never ran: what it started with is forgotten.
pub(crate) fn not_read(app: &mut RustGisApp, command: &str, params: &Value) {
    if let Some(target) = target_of(app, command, params, None) {
        app.diskwatch.monitor.not_read(&target);
    }
}

/// The file a save or export is about to write, if it's watched, and what is there now: taken
/// just before the command runs (one metadata read of one file), so a change another program
/// made before the save is still reported after it.
pub(crate) fn before_write(app: &RustGisApp, command: &str, params: &Value) -> Option<Write> {
    if !app.watch_files {
        return None;
    }
    let path = match command {
        "gpkg.saveEdits" => {
            let id = app.session.layer_id(params).ok()?;
            app.session.map().layer(id)?.gpkg.as_ref()?.path.clone()
        }
        // An existing GeoPackage written into; a new file isn't watched yet.
        "share.exportLayer" | "share.exportTable" => params.get("path").and_then(Value::as_str)?.to_string(),
        _ => return None,
    };
    let key = key(&path);
    app.diskwatch.monitor.watches(&key).then(|| Write { before: look(&path), key, path })
}

/// A write RustGIS is about to make into a watched file ([`before_write`]).
#[derive(Clone)]
pub(crate) struct Write {
    key: String,
    path: String,
    before: Seen,
}

impl Write {
    /// The watched file it writes.
    pub(crate) fn key(&self) -> &str {
        &self.key
    }
}

/// A command the app ran succeeded: a reload acknowledges what it read, a save into a watched
/// file (`before`, from [`before_write`]) is RustGIS's own write.
pub(crate) fn ran(app: &mut RustGisApp, command: &str, params: &Value, answer: &Value, before: Option<Write>) {
    match command {
        "layer.reloadData" | "gpkg.discardEdits" | "map.reloadTiles" => {
            if let Some(target) = target_of(app, command, params, Some(answer)) {
                app.diskwatch.monitor.reloaded(&target);
            }
        }
        // A save that wrote nothing (no edits) isn't a write; an export started on its thread
        // says so as it lands ([`crate::exportjob::land`]).
        "gpkg.saveEdits" if answer.get("saved").and_then(Value::as_bool) == Some(false) => {}
        "share.exportLayer" | "share.exportTable" if answer.get("started").is_some() => {}
        "gpkg.saveEdits" | "share.exportLayer" | "share.exportTable" => {
            if let Some(write) = before {
                wrote(app, write);
            }
        }
        _ => {}
    }
}

/// RustGIS wrote a watched file: what is there now (one metadata read) is its result.
pub(crate) fn wrote(app: &mut RustGisApp, write: Write) {
    let result = look(&write.path);
    let w = &mut app.diskwatch;
    // A scan under way started before the write; the next one is the first after it.
    w.monitor.wrote(&write.key, write.before, result, w.scans + 1);
}

/// Activates the target's map and returns the target's layer id or tile index there.
fn place(app: &mut RustGisApp, target: &Target) -> Result<Place, String> {
    let gone = || tl!("The layer or archive is no longer in the project.").to_string();
    let map = match target {
        Target::Layer { map, .. } | Target::Tiles { map, .. } => *map,
    };
    let at = app.session.project.maps.iter().position(|m| m.instance_id() == map).ok_or_else(gone)?;
    if app.session.project.active_map != at {
        let name = app.session.project.maps.get(at).map(|m| m.name.clone()).ok_or_else(gone)?;
        app.run("map.activate", json!({"name": name}))?;
        if app.session.map().instance_id() != map {
            return Err(tl!("Another map has that name: open the map and use Reload Data from the Layers pane.").into());
        }
    }
    match target {
        Target::Layer { layer, .. } => {
            app.session.map().layers.iter().find(|l| l.instance_id() == *layer).map(|l| Place::Layer(l.id)).ok_or_else(gone)
        }
        Target::Tiles { .. } => {
            let map = app.session.map();
            (0..map.tiles.len()).find(|&i| tiles_target(map, i).as_ref() == Some(target)).map(Place::Tiles).ok_or_else(gone)
        }
    }
}

enum Place {
    Layer(u32),
    Tiles(usize),
}

fn act(app: &mut RustGisApp, target: &Target, repair: bool) {
    let place = match place(app, target) {
        Ok(place) => place,
        Err(e) => {
            app.session.notify(e, true);
            return;
        }
    };
    // Failures say why through the command's own notice; the target stays pending.
    let _ = match (place, repair) {
        (Place::Layer(id), false) => app.run("ui.reloadData", json!({"layer": id})),
        (Place::Tiles(i), false) => app.run("map.reloadTiles", json!({"index": i})),
        (Place::Layer(id), true) => app.run("ui.dialog", json!({"name": "repairSource", "layer": id})),
        (Place::Tiles(i), true) => app.run("ui.dialog", json!({"name": "repairTiles", "index": i})),
    };
}

/// The notification: each source that changed, went missing or can't be checked, with Reload
/// Data (or Repair Data Source) for each layer or archive reading it, and Not Now. It doesn't
/// block the window, and doesn't come back for an observation it was dismissed for.
pub(crate) fn show(app: &mut RustGisApp, ctx: &egui::Context) {
    let w = &app.diskwatch;
    let mut groups: Vec<(&str, &str, Status, Vec<&Found>)> = Vec::new();
    for f in w.found.iter().filter(|f| w.monitor.notifies(&f.target)) {
        match groups.iter_mut().find(|g| g.0 == f.key) {
            Some(g) => g.3.push(f),
            None => groups.push((&f.key, &f.path, w.status(&f.target), vec![f])),
        }
    }
    if groups.is_empty() {
        return;
    }
    let t = Tokens::get(ctx);
    let mut chosen: Option<(Target, bool)> = None;
    let mut dismissed: Vec<Target> = Vec::new();
    egui::Window::new(tl!("Changed on disk"))
        .id(egui::Id::unique("changed_on_disk"))
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -40.0))
        .collapsible(false)
        .resizable(false)
        .default_width(380.0)
        .show(ctx, |ui| {
            for (i, (_, path, status, found)) in groups.iter().enumerate() {
                if i > 0 {
                    ui.separator();
                }
                let file = std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path);
                ui.label(egui::RichText::new(file).strong()).on_hover_text(*path);
                match status {
                    Status::Changed => {
                        ui.label(tl!("The file may have changed since it was read. The map shows the data read before."));
                    }
                    Status::Missing => {
                        ui.colored_label(t.red, tl!("The file is missing. Restore it, or use Repair Data Source."));
                    }
                    Status::Unreadable(e) => {
                        ui.colored_label(t.red, crate::i18n::fmt("The file can't be checked: {error}", &[("error", e)]));
                    }
                    Status::Unchanged | Status::Folder => {}
                }
                for f in found {
                    ui.horizontal(|ui| {
                        ui.label(crate::i18n::fmt("{map} › {layer}", &[("map", &f.map), ("layer", &f.name)]));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let repair = *status == Status::Missing && f.repairable;
                            let label = if repair { tl!("Repair Data Source…") } else { tl!("Reload Data") };
                            if ui.button(label).clicked() {
                                chosen = Some((f.target.clone(), repair));
                            }
                        });
                    });
                }
                if ui
                    .button(tl!("Not Now"))
                    .on_hover_text(tl!("Keep the data shown now. The layer stays marked; a newer change is reported again."))
                    .clicked()
                {
                    dismissed.extend(found.iter().map(|f| f.target.clone()));
                }
            }
        });
    for target in &dismissed {
        app.diskwatch.monitor.dismiss(target);
    }
    if let Some((target, repair)) = chosen {
        act(app, &target, repair);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// `n` distinct observations, of temporary files of different sizes.
    fn observations(n: usize) -> Vec<Seen> {
        // One folder per call: the tests run in parallel.
        static CALL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let call = CALL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("rustgis-diskwatch-{}-{call}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let seen = (0..n)
            .map(|i| {
                let path = dir.join(format!("{i}.fgb"));
                std::fs::write(&path, vec![0; i + 1]).unwrap();
                look(&path.display().to_string())
            })
            .collect();
        std::fs::remove_dir_all(dir).unwrap();
        seen
    }

    fn layer(n: u64) -> Target {
        Target::Layer { map: 1, layer: n }
    }

    /// A monitor with targets on `a.fgb` (layers 1 and 2) and `b.gpkg` (layer 3), all baselined at `v0`.
    fn monitor(v0: &Seen) -> (Monitor, u64) {
        let mut m = Monitor::default();
        assert!(m.track([(layer(1), "a".into()), (layer(2), "a".into()), (layer(3), "b".into())]));
        for t in [1, 2, 3] {
            m.baseline(&layer(t), v0.clone());
        }
        for scan in 1..=2 {
            m.observed("a", v0.clone(), scan);
            m.observed("b", v0.clone(), scan);
        }
        (m, 3)
    }

    #[test]
    fn a_change_is_offered_once_two_scans_agree_and_each_target_clears_alone() {
        let [v0, v1, v2] = observations(3).try_into().unwrap();
        let (mut m, scan) = monitor(&v0);
        assert_eq!(m.status(&layer(1)), Status::Unchanged);
        // A write under way: one scan sees it, the next sees it finished differently.
        m.observed("a", v1.clone(), scan);
        assert_eq!(m.status(&layer(1)), Status::Unchanged, "one scan isn't enough");
        m.observed("a", v2.clone(), scan + 1);
        assert_eq!(m.status(&layer(1)), Status::Unchanged);
        m.observed("a", v2.clone(), scan + 2);
        assert_eq!((m.status(&layer(1)), m.status(&layer(2)), m.status(&layer(3))), (Status::Changed, Status::Changed, Status::Unchanged));
        assert!(m.notifies(&layer(1)) && m.notifies(&layer(2)));
        // Reloading one target leaves the other reading the same file pending.
        m.reading(layer(1), v2.clone());
        m.reloaded(&layer(1));
        assert_eq!((m.status(&layer(1)), m.status(&layer(2))), (Status::Unchanged, Status::Changed));
    }

    #[test]
    fn a_reload_acknowledges_only_what_it_read() {
        let [v0, v1, v2, v3] = observations(4).try_into().unwrap();
        let (mut m, scan) = monitor(&v0);
        m.observed("a", v1.clone(), scan);
        m.observed("a", v1.clone(), scan + 1);
        m.reading(layer(1), v1.clone());
        // Written again while the reload read.
        m.observed("a", v2.clone(), scan + 2);
        m.observed("a", v2.clone(), scan + 3);
        m.reloaded(&layer(1));
        assert_eq!(m.status(&layer(1)), Status::Changed, "the newer change stays pending");
        // A failed reload (no `reloaded`) keeps it pending too; a later success clears it.
        m.reading(layer(1), v2.clone());
        assert_eq!(m.status(&layer(1)), Status::Changed);
        m.reading(layer(1), v2.clone());
        m.reloaded(&layer(1));
        assert_eq!(m.status(&layer(1)), Status::Unchanged);
        // Two reloads queued, the file changing between them: each acknowledges what its own
        // read started with (reads run one at a time).
        m.reading(layer(1), v2.clone());
        m.observed("a", v3.clone(), scan + 4);
        m.observed("a", v3.clone(), scan + 5);
        m.reloaded(&layer(1));
        assert_eq!(m.status(&layer(1)), Status::Changed, "the first read didn't see v3");
        m.reading(layer(1), v3.clone());
        m.reloaded(&layer(1));
        assert_eq!(m.status(&layer(1)), Status::Unchanged);
        // A reload of a change no scan has seen yet (b.gpkg now v2) acknowledges what it read,
        // not what the scans last agreed on (v0).
        m.reading(layer(3), v2.clone());
        m.reloaded(&layer(3));
        m.observed("b", v2.clone(), scan + 6);
        m.observed("b", v2.clone(), scan + 7);
        assert_eq!(m.status(&layer(3)), Status::Unchanged, "it read v2, which then settled");
        // A read started for another target isn't taken: a reload with no observation of its own
        // (run some other way) leaves the mark, since what it read isn't known.
        m.observed("a", v1.clone(), scan + 8);
        m.observed("a", v1.clone(), scan + 9);
        m.reading(layer(2), v3.clone());
        m.reloaded(&layer(1));
        assert_eq!(m.status(&layer(1)), Status::Changed);
        m.reloaded(&layer(2));
        assert_eq!(m.status(&layer(2)), Status::Changed, "layer 2's read started at v3, not v1");
        // A reload before two scans have agreed (a target just added) acknowledges what it read.
        let mut m = Monitor::default();
        m.track([(layer(9), "c".into())]);
        m.observed("c", v0.clone(), 1);
        m.observed("c", v1.clone(), 2);
        m.reading(layer(9), v1.clone());
        m.reloaded(&layer(9));
        m.observed("c", v1.clone(), 3);
        assert_eq!(m.status(&layer(9)), Status::Unchanged, "the early reload read the change");
    }

    #[test]
    fn not_now_is_for_one_observation() {
        let [v0, v1, v2] = observations(3).try_into().unwrap();
        let (mut m, scan) = monitor(&v0);
        m.observed("a", v1.clone(), scan);
        m.observed("a", v1.clone(), scan + 1);
        m.dismiss(&layer(1));
        assert!(!m.notifies(&layer(1)) && m.notifies(&layer(2)), "dismissal is per target");
        assert_eq!(m.status(&layer(1)), Status::Changed, "the layer stays marked");
        m.observed("a", v1.clone(), scan + 2);
        assert!(!m.notifies(&layer(1)), "the same observation again doesn't notify again");
        m.observed("a", v2.clone(), scan + 3);
        m.observed("a", v2.clone(), scan + 4);
        assert!(m.notifies(&layer(1)), "a newer change notifies again");
    }

    #[test]
    fn missing_and_unreadable_are_states_that_clear_on_a_good_check() {
        let [v0, v1] = observations(2).try_into().unwrap();
        let (mut m, scan) = monitor(&v0);
        m.observed("a", Seen::Missing, scan);
        m.observed("a", Seen::Missing, scan + 1);
        assert_eq!(m.status(&layer(1)), Status::Missing);
        assert!(m.notifies(&layer(1)));
        m.dismiss(&layer(1));
        assert!(!m.notifies(&layer(1)), "no repeated prompt while it stays missing");
        m.observed("a", Seen::Unreadable("locked".into()), scan + 2);
        m.observed("a", Seen::Unreadable("locked".into()), scan + 3);
        assert_eq!(m.status(&layer(1)), Status::Unreadable("locked".into()));
        // Back as it was: up to date. Back changed: changed.
        m.observed("a", v0.clone(), scan + 4);
        m.observed("a", v0.clone(), scan + 5);
        assert_eq!(m.status(&layer(1)), Status::Unchanged);
        m.observed("a", Seen::Missing, scan + 6);
        m.observed("a", Seen::Missing, scan + 7);
        m.observed("a", v1.clone(), scan + 8);
        m.observed("a", v1.clone(), scan + 9);
        assert_eq!(m.status(&layer(1)), Status::Changed, "reappearance is detected");
        // A source missing when its target was added changes once it appears.
        let mut m = Monitor::default();
        m.track([(layer(9), "c".into())]);
        m.observed("c", Seen::Missing, 1);
        m.observed("c", v0.clone(), 2);
        m.observed("c", v0.clone(), 3);
        assert_eq!(m.status(&layer(9)), Status::Changed);
    }

    #[test]
    fn rustgis_own_write_is_acknowledged_once_stable_but_an_earlier_change_is_not() {
        let [v0, v1, v2, v3, v4] = observations(5).try_into().unwrap();
        let (mut m, scan) = monitor(&v0);
        // A scan started before the save sees the file mid-write; the save's result (v2) settles.
        m.wrote("b", v0.clone(), v2.clone(), scan + 1);
        m.observed("b", v1.clone(), scan);
        m.observed("b", v2.clone(), scan + 1);
        m.observed("b", v2.clone(), scan + 2);
        assert_eq!(m.status(&layer(3)), Status::Unchanged, "the self-save is not an external change");
        // Saved, then nothing changed on disk at all (the write was a no-op): still acknowledged,
        // and a later external change is still seen.
        m.wrote("b", v2.clone(), v2.clone(), scan + 3);
        m.observed("b", v2.clone(), scan + 3);
        m.observed("b", v2.clone(), scan + 4);
        m.observed("b", v3.clone(), scan + 5);
        m.observed("b", v3.clone(), scan + 6);
        assert_eq!(m.status(&layer(3)), Status::Changed);
        // A change pending before the save stays pending after it.
        m.wrote("b", v3.clone(), v1.clone(), scan + 7);
        m.observed("b", v1.clone(), scan + 7);
        m.observed("b", v1.clone(), scan + 8);
        assert_eq!(m.status(&layer(3)), Status::Changed);
        // An outside change no scan saw (v4), then a save at once: the save doesn't absorb it.
        let (mut m, scan) = monitor(&v0);
        m.wrote("b", v4.clone(), v1.clone(), scan);
        m.observed("b", v1.clone(), scan);
        m.observed("b", v1.clone(), scan + 1);
        assert_eq!(m.status(&layer(3)), Status::Changed, "the outside write is still offered");
        // RustGIS saves (v1), then another program writes (v2) before any scan, or between scans.
        for scans in [[v2.clone(), v2.clone(), v2.clone()], [v1.clone(), v2.clone(), v2.clone()]] {
            let (mut m, scan) = monitor(&v0);
            m.wrote("b", v0.clone(), v1.clone(), scan);
            for (n, seen) in scans.into_iter().enumerate() {
                m.observed("b", seen, scan + n as u64);
            }
            assert_eq!(m.status(&layer(3)), Status::Changed, "a change just after the save is still offered");
        }
        // Two saves in a row continue one write; another program writing between them doesn't.
        let (mut m, scan) = monitor(&v0);
        m.wrote("b", v0.clone(), v1.clone(), scan);
        m.wrote("b", v1.clone(), v2.clone(), scan);
        m.observed("b", v2.clone(), scan);
        m.observed("b", v2.clone(), scan + 1);
        assert_eq!(m.status(&layer(3)), Status::Unchanged);
        let (mut m, scan) = monitor(&v0);
        m.wrote("b", v0.clone(), v1.clone(), scan);
        m.wrote("b", v3.clone(), v2.clone(), scan);
        m.observed("b", v2.clone(), scan);
        m.observed("b", v2.clone(), scan + 1);
        assert_eq!(m.status(&layer(3)), Status::Changed, "the write between the saves is offered");
    }

    #[test]
    fn removed_replaced_or_repaired_targets_start_over_and_late_scans_hit_nothing() {
        let [v0, v1] = observations(2).try_into().unwrap();
        let (mut m, scan) = monitor(&v0);
        m.observed("a", v1.clone(), scan);
        m.observed("a", v1.clone(), scan + 1);
        // Layer 1 repaired to another file, layer 2 removed, layer 4 (a new instance) added.
        assert!(m.track([(layer(1), "moved".into()), (layer(3), "b".into()), (layer(4), "a".into())]));
        assert_eq!(m.status(&layer(2)), Status::Unchanged);
        assert!(!m.notifies(&layer(2)));
        assert_eq!(m.keys(), HashSet::from(["moved".to_string(), "b".to_string(), "a".to_string()]));
        m.observed("moved", v0.clone(), scan + 2);
        m.observed("a", v1.clone(), scan + 2);
        assert_eq!(
            (m.status(&layer(1)), m.status(&layer(4))),
            (Status::Unchanged, Status::Unchanged),
            "new targets baseline at their first scan"
        );
        // A late result for a path nothing reads any more is dropped.
        m.track([(layer(3), "b".into())]);
        m.observed("a", v0.clone(), scan + 3);
        assert!(!m.keys().contains("a"));
    }

    #[test]
    fn two_tile_layers_on_one_archive_are_separate_targets() {
        use rustgis_engine::map::{Map, TileKind, TileLayer};
        let path = std::env::temp_dir().join("rustgis-diskwatch-same.pmtiles").display().to_string();
        let mut map = Map::new("m");
        map.tiles = vec![
            TileLayer::new("first", TileKind::PmTiles, path.clone()),
            TileLayer::new("web", TileKind::Raster, "https://tiles.invalid/{z}/{x}/{y}.png"),
            TileLayer::new("second", TileKind::PmTiles, path.clone()),
        ];
        let [first, web, second] = [0, 1, 2].map(|i| tiles_target(&map, i));
        assert!(web.is_none(), "a web service isn't watched");
        let (Some(first), Some(second)) = (first, second) else { panic!("local archives are targets") };
        assert_ne!(first, second);
        // The second still names the second after the web layer goes.
        map.tiles.remove(1);
        assert_eq!(tiles_target(&map, 1), Some(second));
    }

    #[test]
    fn keys_are_lexical_and_hostile_paths_are_observed_without_panicking() {
        let base = std::env::temp_dir().join("rustgis-diskwatch-key");
        let a = base.join("x").join("..").join("data.fgb").display().to_string();
        let b = base.join(".").join("data.fgb").display().to_string();
        assert_eq!(key(&a), key(&b));
        for hostile in ["", "\0", "/nonexistent/\u{202e}/x.shp", "relative/../../..", &"a".repeat(5000)] {
            let _ = key(hostile);
            assert!(!matches!(look(hostile), Seen::Present(_)), "{hostile:?}");
        }
    }

    /// Runs `n` real scans to completion (the timer isn't waited for).
    fn scan(app: &mut RustGisApp, ctx: &egui::Context, n: usize) {
        for _ in 0..n {
            app.diskwatch.wanted = true;
            tick(app, ctx);
            let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while app.diskwatch.scan.is_some() {
                assert!(std::time::Instant::now() < end, "the scan never landed");
                std::thread::sleep(std::time::Duration::from_millis(5));
                tick(app, ctx);
            }
        }
    }

    #[test]
    fn the_app_marks_an_outside_change_and_clears_it_on_reload_but_not_for_its_own_save() {
        use rustgis_engine::data::{Coord, Dataset, Field, FieldType, Geometry, GeometryType, Value as V, raster::Crs, source::gpkg};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("places.gpkg");
        let write = |to: &std::path::Path, name: &str| {
            let mut data = Dataset::new("places", GeometryType::Point, vec![Field::new("name", FieldType::Text)]);
            data.push(Some(Geometry::Point(Coord::new(1.0, 20.0))), vec![V::Text(name.into())]);
            gpkg::write_in(&data, to, Crs::LonLat).unwrap();
        };
        write(&path, "original");
        let mut app = RustGisApp::new(Session::new(rustgis_engine::Project::default()));
        app.watch_files = true;
        let id = app.run("gpkg.addLayer", json!({"path": path, "table": "places"})).unwrap()["layer"].as_u64().unwrap() as u32;
        let target = Target::Layer { map: app.session.map().instance_id(), layer: app.session.map().layer(id).unwrap().instance_id() };
        let ctx = egui::Context::default();
        scan(&mut app, &ctx, 2);
        assert_eq!(app.diskwatch.status(&target), Status::Unchanged, "opening is not a change");
        // Another program replaces the file.
        let other = dir.path().join("other.gpkg");
        write(&other, "changed outside");
        std::fs::rename(&other, &path).unwrap();
        scan(&mut app, &ctx, 2);
        assert_eq!(app.diskwatch.status(&target), Status::Changed);
        assert!(app.diskwatch.monitor.notifies(&target));
        app.run("ui.reloadData", json!({"layer": id})).unwrap();
        scan(&mut app, &ctx, 2);
        assert_eq!(app.diskwatch.status(&target), Status::Unchanged, "the reload read it");
        // RustGIS's own save into the file.
        app.run("edit.enable", json!({"on": true})).unwrap();
        app.run("edit.setValue", json!({"layer": id, "row": 0, "field": "name", "value": "saved here"})).unwrap();
        app.run("gpkg.saveEdits", json!({"layer": id})).unwrap();
        scan(&mut app, &ctx, 3);
        assert_eq!(app.diskwatch.status(&target), Status::Unchanged, "RustGIS's own write is not an outside change");
    }

    #[test]
    fn a_scan_that_never_answers_is_said_once_and_one_that_answers_isnt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("places.geojson");
        std::fs::write(&path, r#"{"type":"FeatureCollection","features":[]}"#).unwrap();
        let mut app = RustGisApp::new(Session::new(rustgis_engine::Project::default()));
        app.watch_files = true;
        app.run("layer.addData", json!({"path": path, "stream": true})).unwrap();
        let ctx = egui::Context::default();
        let said = |app: &RustGisApp, mark| app.session.notices_since(mark).iter().filter(|n| n.text.contains("doesn't answer")).count();
        // Scans that answer say nothing.
        let mark = app.session.notice_mark();
        scan(&mut app, &ctx, 2);
        assert_eq!(said(&app, mark), 0);
        // A scan in flight for longer than `STALLED` whose slot never fills: said once, and no
        // second scan is started over it.
        let started = web_time::Instant::now().checked_sub(STALLED * 2).unwrap();
        app.diskwatch.scan = Some((app.diskwatch.scans + 1, Scanned::default(), started));
        let scans = app.diskwatch.scans;
        for _ in 0..3 {
            tick(&mut app, &ctx);
        }
        assert_eq!(said(&app, mark), 1);
        assert_eq!(app.diskwatch.scans, scans, "no scan starts while one is in flight");
    }
}
