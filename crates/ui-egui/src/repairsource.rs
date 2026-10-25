//! Repair a layer's linked file or a local tile archive without replacing its configuration.
use crate::RustGisApp;
#[cfg(not(target_arch = "wasm32"))]
use crate::theme::Tokens;
use rustgis_engine::Session;
#[cfg(not(target_arch = "wasm32"))]
use serde_json::json;

/// The map-local item whose linked file is being repaired.
#[derive(Clone, Copy)]
pub enum Target {
    Layer(u32),
    Tiles(usize),
}

/// Captured source details and the replacement selected in Repair Data Source.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub struct Form {
    target: Target,
    opened: u64,
    map: (usize, String),
    /// An index is not an identity: closing on a different archive prevents accidental retargeting.
    archive: Option<(String, rustgis_engine::map::TileKind)>,
    name: String,
    saved: String,
    read_error: Option<String>,
    path: String,
    listed_for: String,
    tables: Vec<String>,
    table: String,
    error: Option<String>,
}

impl Form {
    /// Opens on a specific item, retaining its project and map rather than the current selection.
    pub fn new(s: &Session, target: Target) -> Result<Self, String> {
        let (name, saved, read_error, table) = match target {
            Target::Layer(id) => {
                let l = s.map().layer(id).ok_or_else(|| tl!("Layer not found").to_string())?;
                let saved = l.linked_file().ok_or_else(|| {
                    tl!("This layer's data is kept in the project or read from a database or web address: there's no file to repair.")
                        .to_string()
                })?;
                let read_error =
                    l.raster.as_ref().and_then(|r| r.error.clone()).or_else(|| l.source.as_ref().and_then(|s| s.error.clone()));
                (l.name.clone(), saved.to_string(), read_error, l.linked_table().unwrap_or_default().to_string())
            }
            Target::Tiles(index) => {
                let t = s.map().tiles.get(index).ok_or_else(|| tl!("Tile layer not found").to_string())?;
                let saved = t
                    .archive_file()
                    .ok_or_else(|| tl!("Only a tile archive read from a file (PMTiles or MBTiles) can be repaired.").to_string())?;
                (t.name.clone(), saved.to_string(), t.error.clone(), String::new())
            }
        };
        Ok(Self {
            target,
            opened: s.opened,
            map: (s.project.active_map, s.map().name.clone()),
            archive: match target {
                Target::Layer(_) => None,
                Target::Tiles(index) => s.map().tiles.get(index).map(|t| (t.id.clone(), t.kind)),
            },
            name,
            saved,
            read_error,
            path: String::new(),
            listed_for: String::new(),
            tables: Vec::new(),
            table,
            error: None,
        })
    }
}

/// Shows the captured source and runs the appropriate undoable repair command.
pub fn show(app: &mut RustGisApp, ctx: &egui::Context) {
    let Some(f) = app.dialogs.repair.take() else {
        app.dialogs.open = None;
        return;
    };
    if app.session.opened != f.opened
        || app.session.project.active_map != f.map.0
        || app.session.map().name != f.map.1
        || match f.target {
            Target::Layer(id) => app.session.map().layer(id).is_none(),
            Target::Tiles(index) => app.session.map().tiles.get(index).is_none_or(|t| {
                t.archive_file() != Some(f.saved.as_str()) || f.archive.as_ref().is_none_or(|(id, kind)| t.id != *id || t.kind != *kind)
            }),
        }
    {
        app.dialogs.open = None;
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let mut f = f;
    let mut open = true;
    let mut close = false;
    #[cfg(not(target_arch = "wasm32"))]
    let t = Tokens::get(ctx);
    egui::Window::new(tl!("Repair Data Source"))
        .id(egui::Id::unique("repair_source"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center() + egui::vec2(0.0, -40.0))
        .show(ctx, |ui| {
            ui.set_width(440.0);
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            #[cfg(target_arch = "wasm32")]
            {
                ui.label(tl!("The web app can't link to files on this computer: add the file again with Add Data."));
                close = crate::widgets::dialog_button(ui, tl!("Close"), None, egui::vec2(84.0, 28.0)).clicked();
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                egui::Grid::new("repair_source_details").num_columns(2).spacing(egui::vec2(8.0, 8.0)).show(ui, |ui| {
                    ui.label(tl!("Layer"));
                    ui.add(egui::Label::new(&f.name).wrap());
                    ui.end_row();
                    ui.label(tl!("Saved location"));
                    ui.add(egui::Label::new(egui::RichText::new(&f.saved).monospace()).wrap().selectable(true));
                    ui.end_row();
                    if let Some(error) = &f.read_error {
                        ui.label(tl!("Read error"));
                        ui.add(egui::Label::new(egui::RichText::new(error).color(t.red)).wrap().selectable(true));
                        ui.end_row();
                    }
                });
                ui.separator();
                let replacement_label = ui.label(tl!("Replacement"));
                ui.add(egui::TextEdit::singleline(&mut f.path).desired_width(f32::INFINITY)).labelled_by(replacement_label.id);
                ui.horizontal(|ui| {
                    if ui.button(tl!("Choose File…")).clicked()
                        && let Some(path) = crate::dialogs::pick_open(&[], tl!("Data"))
                    {
                        f.path = path;
                    }
                    if ui.button(tl!("Choose Folder…")).clicked()
                        && let Some(path) = crate::dialogs::pick_folder()
                    {
                        f.path = path;
                    }
                });
                if f.path != f.listed_for && matches!(f.target, Target::Layer(_)) {
                    f.listed_for = f.path.clone();
                    f.tables.clear();
                    f.error = None;
                    if !f.path.trim().is_empty()
                        && let Ok(result) = app.run("info.fileLayers", json!({"path": f.path.trim()}))
                        && result["count"].as_u64().is_some_and(|n| n > 1)
                    {
                        f.tables = result["layers"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect();
                        if !f.tables.contains(&f.table) {
                            f.table.clear();
                        }
                    }
                }
                if !f.tables.is_empty() {
                    let dataset_label = ui.label(tl!("Dataset"));
                    egui::ComboBox::from_id_salt("repair_source_dataset")
                        .selected_text(&f.table)
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            for table in &f.tables {
                                ui.selectable_value(&mut f.table, table.clone(), table);
                            }
                        })
                        .response
                        .labelled_by(dataset_label.id);
                }
                if let Some(error) = &f.error {
                    ui.colored_label(t.red, error);
                }
                ui.separator();
                let mut repair = false;
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        repair = ui
                            .add_enabled_ui(!f.path.trim().is_empty() && (f.tables.is_empty() || !f.table.is_empty()), |ui| {
                                crate::widgets::dialog_button(ui, tl!("Repair"), Some(t.accent), egui::vec2(84.0, 28.0)).clicked()
                            })
                            .inner;
                        close = crate::widgets::dialog_button(ui, tl!("Cancel"), None, egui::vec2(84.0, 28.0)).clicked();
                    });
                });
                if repair {
                    let result = match f.target {
                        Target::Layer(id) => {
                            let mut p = json!({"layer": id, "path": f.path.trim()});
                            if !f.table.is_empty() {
                                p["table"] = json!(f.table);
                            }
                            app.run("layer.repairSource", p)
                        }
                        Target::Tiles(index) => app.run("map.repairTiles", json!({"index": index, "path": f.path.trim()})),
                    };
                    match result {
                        Ok(_) => {
                            app.session.notify(crate::i18n::fmt("Repaired {name}", &[("name", &f.name)]), false);
                            close = true;
                        }
                        Err(error) => f.error = Some(error),
                    }
                }
            }
        });
    if !open || close {
        app.dialogs.open = None;
    } else {
        app.dialogs.repair = Some(f);
    }
}
