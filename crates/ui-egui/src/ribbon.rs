//! The ribbon: the tab strip (with the Project button) and every tab's groups. Each control runs a
//! command by id.

use egui::{Align2, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use rustgis_engine::Tool;
use rustgis_engine::data::GeometryType;
use rustgis_engine::map::planets::EARTH_BASEMAP;
use rustgis_engine::map::view::Weather;
use rustgis_engine::map::{Basemap, RasterRenderer, Renderer, Resampling, Stretch};
use serde_json::{Value, json};

use crate::theme::{Tokens, regular, semibold};
use crate::widgets::{
    CONTENT_H, Dragged, LABEL_H, big, big_form_menu, big_menu, big_split_menu, col, group, menu_item, small, small_form_menu, small_menu,
    submenu_row, text_w,
};
use crate::{RightPane, RustGisApp, icons};

pub const TABS: [&str; 11] = ["Home", "Data", "Style", "Edit", "Analyze", "View", "Publish", "Layout", "Plugins", "Settings", "Help"];

/// Tabs that always show: Settings holds Customize › Ribbon, the way to show hidden tabs again.
pub const FIXED_TABS: [&str; 2] = ["Settings", "Help"];

/// The tab that holds what an older tab name (in saved pane sets, tasks and scripts) held; Home for
/// a name that is no tab. Plugins' tabs pass through.
pub fn current_tab(name: &str) -> &str {
    let tab = match name {
        "Map" => "Home",
        "Insert" | "Raster Data" => "Data",
        "Feature Layer" | "Labeling" | "Raster Layer" => "Style",
        "Analysis" | "Imagery" => "Analyze",
        "Share" => "Publish",
        _ => name,
    };
    if TABS.contains(&tab) || tab.starts_with("plugin:") { tab } else { "Home" }
}

/// Saved hidden tabs under the current names: renamed tabs carry over, and names of tabs whose
/// groups were spread over several others are dropped (they could no longer be shown again), and so
/// are the tabs that always show.
pub fn current_hidden_tabs(saved: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in saved {
        let tab = match name.as_str() {
            "Map" => "Home",
            "Analysis" => "Analyze",
            "Share" => "Publish",
            n => n,
        };
        if (TABS.contains(&tab) || tab.starts_with("plugin:"))
            && tab != "Layout"
            && !FIXED_TABS.contains(&tab)
            && !out.iter().any(|t| t == tab)
        {
            out.push(tab.to_string());
        }
    }
    out
}

/// The tab id of a plugin's own tab.
pub fn plugin_tab_id(plugin: &str) -> String {
    format!("plugin:{plugin}")
}

/// The enabled plugins with a tab of their own: (tab id, caption in the interface language).
fn plugin_tabs(app: &RustGisApp) -> Vec<(String, String)> {
    let lang = app.lang().code();
    rustgis_engine::plugins::list()
        .into_iter()
        .filter(|p| p.enabled)
        .filter_map(|p| p.manifest.tab.as_ref().map(|t| (plugin_tab_id(&p.manifest.id), p.manifest.translate(lang, &t.label).to_string())))
        .collect()
}

pub fn show(app: &mut RustGisApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    if app.ui.tab == "Layout" && app.session.active_layout.is_none() {
        app.ui.tab = "Publish".into();
    }
    // A plugin's tab whose plugin was turned off or removed.
    if app.ui.tab.starts_with("plugin:") && !plugin_tabs(app).iter().any(|(id, _)| *id == app.ui.tab) {
        app.ui.tab = "Plugins".into();
    }
    egui::Panel::top("tabs")
        .exact_size(32.0)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.tab_strip).inner_margin(egui::Margin { left: 8, right: 8, top: 0, bottom: 0 }))
        .show(ui, |ui| tab_strip(app, ui, &t));
    if app.ui.ribbon_minimized && !app.ui.ribbon_peek {
        return;
    }
    // A peeked ribbon closes on the next click outside the tab strip and the ribbon itself.
    // Popups opened from the ribbon (menus, combos, colour pickers) draw outside it, so a click in
    // one must not close it.
    let peek_started = app.ui.ribbon_peek && ui.input(|i| i.pointer.any_click()) && !egui::Popup::is_any_open(ui.ctx());
    let ribbon = egui::Panel::top("ribbon")
        .exact_size(CONTENT_H + LABEL_H + 12.0)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(t.ribbon).inner_margin(egui::Margin { left: 8, right: 8, top: 5, bottom: 3 }))
        .show(ui, |ui| {
            let r = ui.max_rect();
            let full = Rect::from_min_max(pos2(r.min.x - 8.0, r.max.y + 3.0), pos2(r.max.x + 8.0, r.max.y + 4.0));
            ui.painter().rect_filled(full, 0.0, t.border);
            let tips = [tl!("Scroll the ribbon left"), tl!("Scroll the ribbon right")];
            arrow_scroll_area(ui, &t, "ribbon_groups", egui::ScrollArea::horizontal(), tips, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
                    let tab = app.ui.tab.clone();
                    match tab.as_str() {
                        t if t.starts_with("plugin:") => plugin_tab(app, ui, t.trim_start_matches("plugin:")),
                        "Data" => data_tab(app, ui),
                        "Style" => style_tab(app, ui),
                        "Edit" => edit_tab(app, ui),
                        "Analyze" => analyze_tab(app, ui),
                        "View" => view_tab(app, ui),
                        "Publish" => publish_tab(app, ui),
                        "Layout" => layout_groups(app, ui),
                        "Plugins" => plugins_tab(app, ui),
                        "Settings" => settings_tab(app, ui),
                        "Help" => help_tab(app, ui),
                        _ => home_tab(app, ui),
                    }
                });
            });
        });
    if peek_started {
        let pos = ui.input(|i| i.pointer.interact_pos());
        let inside = pos.is_some_and(|p| ribbon.response.rect.contains(p) || app.ui.tab_strip_rect.is_some_and(|r| r.contains(p)));
        if !inside {
            app.ui.ribbon_peek = false;
        }
    }
}

const SCROLL_BUTTON_W: f32 = 18.0;

/// Arrow buttons at the edges of a horizontal scroll area (the ribbon's groups, the tab strip)
/// whose content is wider than the window: the scroll bar is hidden (it would cover the group
/// labels and tabs) and the wheel alone is undiscoverable.
///
/// The offset a click asked for, if one did.
fn scroll_buttons<R>(ui: &mut Ui, t: &Tokens, scroll: &egui::scroll_area::ScrollAreaOutput<R>, tips: [&str; 2]) -> Option<f32> {
    let view = scroll.inner_rect;
    let max = (scroll.content_size.x - view.width()).max(0.0);
    if max < 1.0 {
        return None;
    }
    let offset = scroll.state.clamped_offset().x;
    let step = (view.width() * 0.6).max(120.0);
    let mut moved = None;
    for (left, show) in [(true, offset > 0.5), (false, offset < max - 0.5)] {
        if !show {
            continue;
        }
        let x = if left { view.min.x } else { view.max.x - SCROLL_BUTTON_W };
        let r = Rect::from_min_size(pos2(x, view.min.y), vec2(SCROLL_BUTTON_W, view.height()));
        let resp = ui.interact(r, scroll.id.with(left), Sense::click());
        let fill = if resp.hovered() { t.hover } else { t.ribbon };
        ui.painter().rect_filled(r, CornerRadius::same(4), fill);
        ui.painter().rect_stroke(r, CornerRadius::same(4), Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        let icon = if left { "chevron_left" } else { "chevron_right" };
        let ir = Rect::from_center_size(r.center(), vec2(16.0, 16.0));
        icons::paint(ui.painter(), ir, icon, t.text_dim, t.accent);
        let tip = if left { tips[0] } else { tips[1] };
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
        if resp.on_hover_text(tip).clicked() {
            moved = Some(if left { (offset - step).max(0.0) } else { (offset + step).min(max) });
        }
    }
    moved
}

/// A horizontal scroll area (`area`, named `name`) without a scroll bar, with [`scroll_buttons`]
/// at its edges.
fn arrow_scroll_area<R>(ui: &mut Ui, t: &Tokens, name: &str, area: egui::ScrollArea, tips: [&str; 2], add: impl FnOnce(&mut Ui) -> R) {
    // egui sets a scroll area's offset only as it is built, so a click moves it on the next frame.
    // It is asked for once, on a frame egui keeps: a discarded one doesn't move the area, and asking
    // on every frame would pin the area there against the wheel.
    let key = ui.make_persistent_id(("arrow_scroll_offset", name));
    let mut area = area.scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
    let pending = ui.data(|d| d.get_temp::<f32>(key));
    if let Some(x) = pending {
        area = area.horizontal_scroll_offset(x);
    }
    let scroll = area.show(ui, add);
    if pending.is_some() && !ui.ctx().will_discard() {
        ui.data_mut(|d| d.remove_temp::<f32>(key));
    }
    if let Some(x) = scroll_buttons(ui, t, &scroll, tips) {
        ui.data_mut(|d| d.insert_temp(key, x));
        ui.ctx().request_repaint();
    }
}

/// The collapse button at the right of the tab strip.
const COLLAPSE_W: f32 = 24.0;

/// The active tool indicator at the right of the tab strip, unless the Explore tool is on: its
/// label and width.
fn tool_indicator(app: &RustGisApp, ui: &Ui) -> Option<(String, f32)> {
    let tool = app.session.tool;
    if tool == Tool::Explore {
        return None;
    }
    let label = crate::i18n::fmt("{tool} tool active · Esc to stop", &[("tool", tl_raw!(tool.label()))]);
    let w = text_w(ui, &label, regular(11.5)) + 18.0;
    Some((label, w))
}

fn tab_strip(app: &mut RustGisApp, ui: &mut Ui, t: &Tokens) {
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
        // The Project button opens the backstage.
        let pw = text_w(ui, tl!("Project"), semibold(12.5)) + 26.0;
        let (pr, presp) = ui.allocate_exact_size(vec2(pw, 26.0), Sense::click());
        let fill = if presp.hovered() { crate::theme::BRAND_DARK } else { crate::theme::BRAND };
        ui.painter().rect_filled(pr, CornerRadius::same(6), fill);
        ui.painter().text(pr.center(), Align2::CENTER_CENTER, tl!("Project"), semibold(12.5), egui::Color32::WHITE);
        if presp.on_hover_text(tl!("Open, save and create projects")).clicked() {
            app.ui.backstage = true;
        }
        ui.add_space(6.0);
        // RustGIS's tabs, then plugins' own tabs; the user can hide any of them.
        let mut tabs: Vec<(String, String)> = TABS.iter().map(|t| (t.to_string(), tl!(*t).to_string())).collect();
        tabs.extend(plugin_tabs(app));
        tabs.retain(
            |(id, _)| {
                if id == "Layout" { app.session.active_layout.is_some() } else { !app.ui.hidden_tabs.iter().any(|h| h == id) }
            },
        );
        // The tabs scroll between the Project button and the right-side controls (the collapse
        // button and the active tool indicator) when the window is too narrow for them.
        let tool = tool_indicator(app, ui);
        let tool_w = tool.as_ref().map_or(0.0, |(_, w)| w + ui.spacing().item_spacing.x);
        let room = (ui.available_width() - COLLAPSE_W - 8.0 - tool_w).max(60.0);
        // Tighter tabs when the window is too narrow for the usual padding.
        let text: f32 = tabs.iter().map(|(_, c)| text_w(ui, c, semibold(12.5))).sum();
        let pad = ((room - text) / tabs.len().max(1) as f32).clamp(8.0, 22.0);
        // The active tab scrolls into view for a few frames (a pass that egui discards drops its
        // scroll request) after it changes, or the strip's room or tab widths do.
        let active_id = egui::Id::unique("tab_strip_active");
        let key = (app.ui.tab.clone(), room.round() as i32, text.round() as i32);
        let (shown, scrolls): ((String, i32, i32), u8) = ui.data(|d| d.get_temp(active_id)).unwrap_or_default();
        let scrolls = if shown == key { scrolls } else { 0 };
        if scrolls < 3 {
            ui.data_mut(|d| d.insert_temp(active_id, (key, scrolls + 1)));
        }
        let area = egui::ScrollArea::horizontal().id_salt("tab_strip").max_width(room);
        let tips = [tl!("Scroll the tabs left"), tl!("Scroll the tabs right")];
        arrow_scroll_area(ui, t, "tab_strip", area, tips, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
                for (tab, caption) in &tabs {
                    let tab = tab.as_str();
                    let active = app.ui.tab == tab;
                    let font = if active { semibold(12.5) } else { regular(12.5) };
                    let shown = caption.as_str();
                    let w = text_w(ui, shown, font.clone()) + pad;
                    let (r, resp) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
                    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, active, shown));
                    let body = Rect::from_min_max(pos2(r.min.x, r.min.y + 3.0), r.max);
                    if active {
                        ui.painter().rect_filled(body, CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, t.ribbon);
                        ui.painter().rect_stroke(
                            body,
                            CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 },
                            Stroke::new(1.0, t.border),
                            egui::StrokeKind::Inside,
                        );
                        ui.painter().rect_filled(
                            Rect::from_min_max(pos2(r.min.x + 1.0, r.max.y - 1.5), pos2(r.max.x - 1.0, r.max.y + 1.0)),
                            0.0,
                            t.ribbon,
                        );
                        let bar = t.accent;
                        ui.painter().rect_filled(
                            Rect::from_min_max(pos2(r.min.x + 8.0, r.max.y - 3.0), pos2(r.max.x - 8.0, r.max.y - 1.0)),
                            CornerRadius::same(1),
                            bar,
                        );
                    } else if resp.hovered() {
                        ui.painter().rect_filled(body.shrink2(vec2(1.0, 2.0)), CornerRadius::same(5), t.hover);
                    }
                    let color = if active { t.text } else { t.text_dim };
                    ui.painter().text(pos2(r.center().x, r.center().y + 1.5), Align2::CENTER_CENTER, shown, font, color);
                    if resp.double_clicked() {
                        let _ = app.run("ui.ribbon", Value::Null);
                    } else if resp.clicked() {
                        if app.ui.ribbon_minimized {
                            app.ui.ribbon_peek = !(app.ui.ribbon_peek && app.ui.tab == tab);
                        }
                        app.ui.tab = tab.to_string();
                    }
                    // A tab that became active (clicked, or opened by a command) scrolls
                    // into view, clear of the arrow buttons.
                    if active && scrolls < 3 {
                        ui.scroll_to_rect_animation(r.expand2(vec2(SCROLL_BUTTON_W, 0.0)), None, egui::style::ScrollAnimation::none());
                    }
                }
            });
        });
        app.ui.tab_strip_rect = Some(ui.max_rect());
        // Right side: collapse/expand the ribbon, then the quick tool indicator.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (cr, cresp) = ui.allocate_exact_size(vec2(COLLAPSE_W, COLLAPSE_W), Sense::click());
            if cresp.hovered() {
                ui.painter().rect_filled(cr, CornerRadius::same(5), t.hover);
            }
            let icon = if app.ui.ribbon_minimized { "chevron_down" } else { "up" };
            icons::paint(ui.painter(), cr.shrink(5.0), icon, t.text_dim, t.accent);
            let tip = if app.ui.ribbon_minimized { tl!("Expand the ribbon (Ctrl+F1)") } else { tl!("Collapse the ribbon (Ctrl+F1)") };
            if cresp.on_hover_text(tip).clicked() {
                let _ = app.run("ui.ribbon", Value::Null);
            }
            if let Some((label, w)) = tool {
                let (r, resp) = ui.allocate_exact_size(vec2(w, 22.0), Sense::click());
                ui.painter().rect_filled(r, CornerRadius::same(11), t.checked);
                ui.painter().text(r.center(), Align2::CENTER_CENTER, label, regular(11.5), t.accent_text);
                if resp.clicked() {
                    app.session.tool = Tool::Explore;
                    app.map.cancel_sketch();
                }
            }
        });
    });
}

fn tool_btn(ui: &mut Ui, app: &mut RustGisApp, icon: &str, label: &str, desc: &str, tool: Tool) {
    let checked = app.session.tool == tool;
    let r = big(ui, app, icon, label, desc, "tool.set", json!({"tool": tool.code()}), checked, false);
    if r.clicked() {
        app.map.cancel_sketch();
    }
}

/// The Clipboard group of the Edit tab.
fn clipboard_group(app: &mut RustGisApp, ui: &mut Ui) {
    group(ui, "Clipboard", |ui| {
        big_split_menu(
            ui,
            app,
            "paste",
            "Paste",
            "Paste copied features into the selected layer.",
            "edit.paste",
            Value::Null,
            false,
            "Paste options",
            "Paste features into the selected layer or a new layer.",
            true,
            |ui, app| {
                menu_item(ui, app, "paste", "Paste Special (as a new layer)", "edit.pasteSpecial", Value::Null);
            },
        );
        col(ui, |ui| {
            small(ui, app, "cut", "Cut", "Copy the selected features and delete them.", "ui.cut", Value::Null, false);
            small(ui, app, "copy", "Copy", "Copy the selected features (also as GeoJSON text).", "ui.copy", Value::Null, false);
            small(ui, app, "copy_path", "Copy Path", "Copy the selected layer's data source path.", "ui.copyPath", Value::Null, false);
        });
    });
}

fn home_tab(app: &mut RustGisApp, ui: &mut Ui) {
    // New maps and layouts and the project's items first: where work starts (#328).
    project_group(app, ui);
    group(ui, "Navigate", |ui| {
        tool_btn(
            ui,
            app,
            "explore",
            "Pan",
            "Pan with the mouse, zoom with the wheel and click features to see their attributes.",
            Tool::Explore,
        );
        big_menu(ui, app, "bookmark", "Bookmarks", "Go to a saved extent or save the current one.", |ui, app| {
            let names: Vec<String> = app.session.map().bookmarks.iter().map(|b| b.name.clone()).collect();
            if names.is_empty() {
                ui.label(egui::RichText::new(tl!("No bookmarks yet")).weak());
            }
            for n in names {
                // The name goes to the bookmark; the button at the right removes it.
                let (go, remove) = egui::Sides::new().show(
                    ui,
                    |ui| ui.add(egui::Button::new(n.as_str()).frame(false).min_size(vec2(0.0, 26.0))).clicked(),
                    |ui| {
                        let tip = crate::i18n::fmt("Remove {name}", &[("name", &n)]);
                        let r = crate::widgets::icon_button(ui, "delete", &tip);
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &tip));
                        r.clicked()
                    },
                );
                if go {
                    let _ = app.run("bookmark.go", json!({"name": n}));
                    ui.close();
                }
                if remove {
                    let _ = app.run("bookmark.remove", json!({"name": n}));
                }
            }
            ui.separator();
            menu_item(ui, app, "bookmark_new", "New Bookmark…", "ui.dialog", json!({"name": "bookmark"}));
            // Every map's bookmarks, renamed, described, updated and ordered in place (#351).
            menu_item(ui, app, "pane_right", "Manage Bookmarks", "ui.pane.bookmarks", Value::Null);
            // The files apart from the bookmarks themselves.
            ui.separator();
            menu_item(ui, app, "bookmark_import", "Import Bookmarks…", "ui.openItem", json!({"kind": "bookmarks"}));
            menu_item(ui, app, "bookmark_export", "Export Bookmarks…", "ui.saveItem", json!({"kind": "bookmarks"}));
        });
        col(ui, |ui| {
            small(ui, app, "zoom_full", "Full Extent", "Zoom to the extent of all layers.", "map.zoomFull", Value::Null, false);
            small(ui, app, "zoom_in", "Zoom In", "Zoom in on the centre of the map.", "map.zoomIn", Value::Null, false);
            small(ui, app, "zoom_out", "Zoom Out", "Zoom out from the centre of the map.", "map.zoomOut", Value::Null, false);
        });
        col(ui, |ui| {
            small(ui, app, "prev_extent", "Previous", "Go back to the previous extent.", "map.prevExtent", Value::Null, false);
            small(ui, app, "next_extent", "Next", "Go forward to the next extent.", "map.nextExtent", Value::Null, false);
            small(
                ui,
                app,
                "zoom_box",
                "Zoom Box",
                "Drag a box to zoom into.",
                "tool.set",
                json!({"tool": "zoomIn"}),
                app.session.tool == Tool::ZoomIn,
            );
        });
        col(ui, |ui| {
            small(ui, app, "goto_xy", "Go To XY", "Pan to a longitude/latitude.", "ui.dialog", json!({"name": "goToXY"}), false);
            small(
                ui,
                app,
                "search",
                "Find Place",
                "Find a place or an address by name (OpenStreetMap).",
                "ui.dialog",
                json!({"name": "findPlace"}),
                false,
            );
            small(
                ui,
                app,
                "directions",
                "Directions",
                "Plan a route between places.",
                "ui.pane.directions",
                Value::Null,
                app.ui.right == Some(RightPane::Directions),
            );
        });
    });
    group(ui, "Add", |ui| {
        big_menu(ui, app, "basemap", "Basemap", "Change the map's backdrop.", basemap_gallery);
        big_split_menu(
            ui,
            app,
            "add_data",
            "Add\nData",
            "Add GeoJSON, CSV or Shapefile data to the map.",
            "ui.addData",
            Value::Null,
            false,
            "Add Data options",
            "Choose a data source or create a layer.",
            true,
            |ui, app| {
                menu_item(ui, app, "", "Delimited Text…", "ui.addDelimited", Value::Null);
                menu_item(ui, app, "", "Geocode Addresses…", "ui.dialog", json!({"name": "geocode"}));
                menu_item(ui, app, "", "From URL…", "ui.addUrl", Value::Null);
                menu_item(ui, app, "", "From Service…", "ui.addService", Value::Null);
                menu_item(ui, app, "", "Zarr…", "ui.dialog", json!({"name": "zarr"}));
                // Databases are read through DuckDB, a program the browser can't run.
                #[cfg(not(target_arch = "wasm32"))]
                menu_item(ui, app, "", "Database…", "ui.dialog", json!({"name": "addDatabase"}));
                menu_item(ui, app, "", "S3…", "ui.dialog", json!({"name": "s3"}));
                // Saved connections live in the Data Sources pane, not here: one entry opens the pane
                // (never closes it), and New Connection starts the form for a kind.
                menu_item(ui, app, "", "Connections…", "ui.openSources", Value::Null);
                submenu_row(ui, "", tl!("New Connection"), |ui| {
                    for (kind, label) in crate::connectionui::KINDS {
                        // Database drivers aren't built for the browser.
                        if cfg!(target_arch = "wasm32") && rustgis_engine::connections::DATABASE_KINDS.contains(&kind) {
                            continue;
                        }
                        menu_item(ui, app, "", label, "ui.connection", json!({"kind": kind}));
                    }
                });
                menu_item(ui, app, "graphics_layer", "Graphics Layer", "graphics.newLayer", Value::Null);
                submenu_row(ui, "graphics_layer", tl!("Layer Templates"), |ui| layer_templates_gallery(ui, app));
                menu_item(ui, app, "add_folder", "Add Folder…", "ui.addFolder", Value::Null);
                ui.separator();
                sample_data_gallery(ui, app);
            },
        );
        big_menu(ui, app, "web_data", "Web\nData", "Add data from public web services and catalogs.", web_data_menu);
    });
    group(ui, "Selection", |ui| {
        tool_btn(ui, app, "select", "Select", "Click or drag a box to select features. Shift adds, Ctrl removes.", Tool::Select);
        big(
            ui,
            app,
            "select_attr",
            "Select By\nAttributes",
            "Select features with a SQL expression.",
            "ui.dialog",
            json!({"name": "selectByAttributes"}),
            false,
            false,
        );
        big(
            ui,
            app,
            "select_loc",
            "Select By\nLocation",
            "Select features that intersect another layer.",
            "ui.gp.open",
            json!({"tool": "selectByLocation"}),
            false,
            false,
        );
        col(ui, |ui| {
            small(ui, app, "clear_sel", "Clear", "Clear the selection in every layer.", "select.clear", Value::Null, false);
            small(ui, app, "zoom_sel", "Zoom To", "Zoom to the selected features.", "select.zoomTo", Value::Null, false);
            small(ui, app, "table", "Attributes", "Open the attribute table of the selected layer.", "table.open", Value::Null, false);
        });
    });
    group(ui, "Inspect", |ui| {
        col(ui, |ui| {
            for (label, code) in [("Measure Distance", "measure"), ("Measure Area", "measureArea")] {
                if small(
                    ui,
                    app,
                    "measure",
                    label,
                    "Measure geodesic distances or areas on the map.",
                    "tool.set",
                    json!({"tool": code}),
                    app.session.tool.code() == code,
                )
                .clicked()
                {
                    app.map.cancel_sketch();
                }
            }
            small(
                ui,
                app,
                "chart",
                "Infographics",
                "Counts, totals and top categories of what's in view.",
                "ui.pane.infographics",
                Value::Null,
                app.ui.right == Some(RightPane::Infographics),
            );
        });
    });
}

/// The Web Data menu: public web services and catalogs, the ones covering the world in one
/// alphabetical list (a service fits one place, where a list by topic would leave it in several).
/// Services covering one country go in a submenu named after it.
fn web_data_menu(ui: &mut Ui, app: &mut RustGisApp) {
    let svc = |id: &str| json!({"service": id});
    let pane = |name: &str| json!({"name": name});
    menu_item(ui, app, "", "ArcGIS Portal…", "ui.searchOpenData", svc("arcgis-portal"));
    menu_item(ui, app, "", "Boundaries…", "ui.dialog", pane("boundaries"));
    menu_item(ui, app, "", "Dynamical…", "ui.searchOpenData", svc("dynamical"));
    menu_item(ui, app, "", "Earthdata GIS…", "ui.searchOpenData", svc("earthdata-gis"));
    menu_item(ui, app, "", "Fields of the World…", "ui.dialog", pane("fieldsOfTheWorld"));
    menu_item(ui, app, "", "GeoLens…", "ui.searchOpenData", svc("geolens"));
    menu_item(ui, app, "", "Hugging Face…", "ui.searchOpenData", svc("huggingface"));
    menu_item(ui, app, "", "NASA Earthaccess…", "ui.dialog", pane("earthaccess"));
    menu_item(ui, app, "", "NASA Earthdata…", "ui.dialog", pane("nasaEarthdata"));
    menu_item(ui, app, "", "Natural Earth…", "ui.dialog", pane("naturalEarth"));
    menu_item(ui, app, "", "Ocean Data Platform…", "ui.searchOpenData", svc("ocean-data"));
    menu_item(ui, app, "", "OpenAerialMap…", "ui.searchOpenData", svc("openaerialmap"));
    menu_item(ui, app, "", "OpenStreetMap…", "ui.dialog", pane("osm"));
    menu_item(ui, app, "", "Overture Maps…", "ui.dialog", pane("overture"));
    menu_item(ui, app, "", "Planet Open Data…", "ui.stac.planet", Value::Null);
    menu_item(ui, app, "", "Portolan…", "ui.stac.portolan", Value::Null);
    menu_item(ui, app, "", "Satellite Embeddings…", "ui.dialog", pane("embeddings"));
    menu_item(ui, app, "", "Search Open Data…", "ui.searchOpenData", Value::Null);
    menu_item(ui, app, "", "Sentinel-2 Explorer…", "ui.stac.sentinel2", Value::Null);
    menu_item(ui, app, "", "Source Cooperative…", "ui.dialog", pane("sourcecoop"));
    menu_item(ui, app, "", "STAC Search…", "ui.stac", Value::Null);
    menu_item(ui, app, "", "Vantor…", "ui.stac.vantor", Value::Null);
    ui.separator();
    submenu_row(ui, "", tl!("France"), |ui| {
        menu_item(ui, app, "", "IGN LiDAR HD…", "ui.searchOpenData", svc("ign-lidar"));
    });
    submenu_row(ui, "", tl!("United States"), |ui| {
        menu_item(ui, app, "", "EnviroAtlas…", "ui.dialog", pane("enviroAtlas"));
        menu_item(ui, app, "", "FEMA NFHL…", "ui.dialog", pane("femaNfhl"));
        menu_item(ui, app, "", "Tennessee GIS…", "ui.searchOpenData", svc("tennessee"));
        menu_item(ui, app, "", "The National Map…", "ui.dialog", pane("nationalMap"));
        menu_item(ui, app, "", "US Federal GIS…", "ui.searchOpenData", svc("us-federal"));
        menu_item(ui, app, "", "US Local GIS…", "ui.searchOpenData", svc("us-local"));
        menu_item(ui, app, "", "US State GIS…", "ui.searchOpenData", svc("us-state"));
        menu_item(ui, app, "", "USGS 3DEP DEM…", "ui.searchOpenData", svc("usgs-dem"));
        menu_item(ui, app, "", "USGS LiDAR…", "ui.stac.usgsLidar", Value::Null);
        menu_item(ui, app, "", "USGS NLDI…", "ui.dialog", pane("nldi"));
    });
}

pub(crate) fn sample_data_gallery(ui: &mut Ui, app: &mut RustGisApp) {
    ui.label(egui::RichText::new(tl!("Sample data (Natural Earth)")).small().weak());
    for (name, label) in
        [("countries", "World Countries"), ("cities", "Populated Places"), ("rivers", "Major Rivers"), ("lakes", "Major Lakes")]
    {
        menu_item(ui, app, "", label, "layer.addSample", json!({"name": name}));
    }
}

pub(crate) fn basemap_gallery(ui: &mut Ui, app: &mut RustGisApp) {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(tl!("Basemaps")).font(semibold(12.5)));
    ui.add_space(4.0);
    egui::Grid::new("basemaps").spacing(vec2(8.0, 8.0)).show(ui, |ui| {
        for (i, b) in Basemap::ALL.into_iter().enumerate() {
            let (water, land, outline, _) = b.palette();
            let (r, resp) = ui.allocate_exact_size(vec2(92.0, 74.0), Sense::click());
            // Web basemaps cover the built-in one.
            let sel = app.session.map().basemap == b && app.session.map().tiles.iter().all(|t| t.overlay);
            resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, sel, tl!(b.label())));
            let thumb = Rect::from_min_size(r.min + vec2(4.0, 4.0), vec2(84.0, 48.0));
            ui.painter().rect_filled(thumb, CornerRadius::same(4), crate::theme::c32(water));
            // A little continent.
            let pts = [(0.18, 0.3), (0.42, 0.18), (0.6, 0.35), (0.55, 0.7), (0.3, 0.82), (0.15, 0.6)];
            let poly: Vec<egui::Pos2> =
                pts.iter().map(|(x, y)| pos2(thumb.min.x + x * thumb.width(), thumb.min.y + y * thumb.height())).collect();
            ui.painter().add(egui::Shape::convex_polygon(poly, crate::theme::c32(land), Stroke::new(1.0, crate::theme::c32(outline))));
            let poly2: Vec<egui::Pos2> = [(0.68, 0.22), (0.88, 0.3), (0.84, 0.55), (0.7, 0.5)]
                .iter()
                .map(|(x, y)| pos2(thumb.min.x + x * thumb.width(), thumb.min.y + y * thumb.height()))
                .collect();
            ui.painter().add(egui::Shape::convex_polygon(poly2, crate::theme::c32(land), Stroke::new(1.0, crate::theme::c32(outline))));
            if sel || resp.hovered() {
                ui.painter().rect_stroke(
                    r,
                    CornerRadius::same(6),
                    Stroke::new(if sel { 2.0 } else { 1.0 }, t.accent),
                    egui::StrokeKind::Inside,
                );
            }
            ui.painter().text(pos2(r.center().x, r.max.y - 11.0), Align2::CENTER_CENTER, tl!(b.label()), regular(11.5), t.text);
            if resp.clicked() {
                let _ = app.run("map.basemap", json!({"name": b.code()}));
                ui.close();
            }
            if i % 3 == 2 {
                ui.end_row();
            }
        }
    });
    web_basemaps(ui, app, &t);
    planet_basemaps(ui, app);
    tile_layers(ui, app, &t);
}

/// The celestial body switcher's menu (the Layers pane's planet button): picking a body moves the
/// map there, with one of its basemaps; the Earth selects USGS Imagery.
pub fn body_menu(ui: &mut Ui, app: &mut RustGisApp) {
    ui.label(egui::RichText::new(tl!("Celestial Body")).font(semibold(12.0)));
    let current = app.session.map().body().id;
    for b in rustgis_engine::geom::BODIES {
        if ui.add(egui::Button::selectable(b.id == current, tl!(b.name))).clicked() {
            let _ = app.run("map.body", json!({"name": b.id}));
            ui.close();
        }
    }
}

/// The celestial bodies, and the basemaps of the one the map is on (#223).
fn planet_basemaps(ui: &mut Ui, app: &mut RustGisApp) {
    ui.add_space(6.0);
    ui.separator();
    ui.label(egui::RichText::new(tl!("Celestial Bodies")).font(semibold(12.5)));
    ui.add_space(2.0);
    let body = app.session.map().body();
    ui.horizontal_wrapped(|ui| {
        ui.set_max_width(300.0);
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for b in rustgis_engine::geom::BODIES {
            let tip = tl!("Measure on this body, with one of its basemaps.");
            if ui.add(egui::Button::selectable(b.id == body.id, tl!(b.name))).on_hover_text(tip).clicked() {
                let _ = app.run("map.body", json!({"name": b.id}));
            }
        }
    });
    if body.is_earth() {
        return;
    }
    ui.add_space(4.0);
    let shown: Vec<String> = app.session.map().tiles.iter().filter(|l| !l.overlay && l.draws()).map(|l| l.id.clone()).collect();
    egui::Grid::new("planet-basemaps").spacing(vec2(6.0, 4.0)).show(ui, |ui| {
        for (i, m) in rustgis_engine::map::planets::basemaps_of(body.id).enumerate() {
            let sel = shown.first().map(String::as_str) == Some(m.id);
            let resp = ui.add_sized(vec2(140.0, 22.0), egui::Button::selectable(sel, m.name).truncate()).on_hover_text(m.attribution);
            if resp.clicked() {
                let _ = app.run("map.body", json!({"name": body.id, "basemap": m.id}));
                ui.close();
            }
            if i % 2 == 1 {
                ui.end_row();
            }
        }
    });
}

const OPENFREEMAP: &str = "OpenFreeMap © OpenMapTiles Data from OpenStreetMap";

/// Web basemaps that need no API key, offered next to the built-in ones:
/// (id, name, kind, url, attribution, max zoom).
const WEB_BASEMAPS: [(&str, &str, &str, &str, &str, u8); 6] = [
    ("openfreemap-liberty", "OpenFreeMap Liberty", "style", "https://tiles.openfreemap.org/styles/liberty", OPENFREEMAP, 14),
    ("openfreemap-bright", "OpenFreeMap Bright", "style", "https://tiles.openfreemap.org/styles/bright", OPENFREEMAP, 14),
    ("openfreemap-positron", "OpenFreeMap Positron", "style", "https://tiles.openfreemap.org/styles/positron", OPENFREEMAP, 14),
    ("openfreemap-dark", "OpenFreeMap Dark", "style", "https://tiles.openfreemap.org/styles/dark", OPENFREEMAP, 14),
    ("openfreemap-fiord", "OpenFreeMap Fiord", "style", "https://tiles.openfreemap.org/styles/fiord", OPENFREEMAP, 14),
    (EARTH_BASEMAP.id, EARTH_BASEMAP.name, "raster", EARTH_BASEMAP.url, EARTH_BASEMAP.attribution, EARTH_BASEMAP.max_zoom),
];

/// The id of the registry plugin with many more web basemaps.
const BASEMAPS_PLUGIN: &str = "basemaps";

/// Web basemaps (OpenFreeMap, USGS) that replace the other tile basemaps or stack on them, and
/// where to get more.
fn web_basemaps(ui: &mut Ui, app: &mut RustGisApp, t: &Tokens) {
    ui.add_space(6.0);
    ui.separator();
    ui.label(egui::RichText::new(tl!("Web Basemaps")).font(semibold(12.5)));
    ui.add_space(2.0);
    // Stacked: a web basemap goes over the others (to blend with them in the Layers pane)
    // rather than replacing them.
    let stack_id = egui::Id::unique("web-basemaps-stack");
    let mut stack = ui.data_mut(|d| *d.get_persisted_mut_or_default::<bool>(stack_id));
    let resp = ui.checkbox(&mut stack, tl!("Stack on the current basemaps"));
    let tip =
        tl!("Add a web basemap over the others instead of replacing them, to blend it with them. Click one that shows to take it off.");
    if resp.on_hover_text(tip).changed() {
        ui.data_mut(|d| d.insert_persisted(stack_id, stack));
    }
    // The web basemaps showing: the one on top, or every one when stacking.
    let shown: Vec<String> = app.session.map().tiles.iter().filter(|l| !l.overlay && l.draws()).map(|l| l.id.clone()).collect();
    egui::Grid::new("web-basemaps").spacing(vec2(6.0, 4.0)).show(ui, |ui| {
        for (i, (id, name, kind, url, attribution, max_zoom)) in WEB_BASEMAPS.into_iter().enumerate() {
            let sel = if stack { shown.iter().any(|s| s == id) } else { shown.first().map(String::as_str) == Some(id) };
            let resp = ui.add_sized(vec2(140.0, 22.0), egui::Button::selectable(sel, name)).on_hover_text(url);
            if resp.clicked() {
                // Stacking, a shown basemap's button takes it off, keeping the others' blending;
                // adding it again would replace it with a fresh layer on top.
                let _ = if stack && sel {
                    app.run("map.removeTiles", json!({"id": id}))
                } else {
                    let p = json!({"url": url, "kind": kind, "name": name, "attribution": attribution, "maxZoom": max_zoom, "id": id, "replace": !stack});
                    app.run("map.addTiles", p)
                };
                ui.close();
            }
            if i % 2 == 1 {
                ui.end_row();
            }
        }
    });
    ui.add_space(4.0);
    // The plugin's tab when it shows; else the Plugin Manager, to turn the plugin on or install it.
    let tab = plugin_tab_id(BASEMAPS_PLUGIN);
    let (ask, link, hint, then) = if plugin_tabs(app).iter().any(|(id, _)| *id == tab) {
        (tl!("More basemaps:"), tl!("Open the Basemaps tab"), tl!("The Basemaps plugin's tab of public basemaps."), None)
    } else if rustgis_engine::plugins::get(BASEMAPS_PLUGIN).is_some() {
        (
            tl!("More basemaps:"),
            tl!("Turn on the Basemaps plugin"),
            tl!("It's installed but turned off: turn it on in the Plugin Manager."),
            Some(json!({"open": true, "tab": "installed"})),
        )
    } else {
        (
            tl!("Want more basemaps?"),
            tl!("Install the Basemaps plugin"),
            tl!("OpenStreetMap, Esri, USGS, NASA and more, from the plugin registry."),
            Some(json!({"open": true, "tab": "browse", "filter": BASEMAPS_PLUGIN})),
        )
    };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(egui::RichText::new(ask).weak().small());
        let resp = ui.add(egui::Link::new(egui::RichText::new(link).small().color(t.accent)));
        if resp.on_hover_text(hint).clicked() {
            match then {
                Some(p) => {
                    let _ = app.run("ui.plugins", p);
                }
                None => app.ui.tab = tab.clone(),
            }
            ui.close();
        }
    });
}

/// The map's web tile layers (top first): shown, opacity, order and removal, and adding one.
fn tile_layers(ui: &mut Ui, app: &mut RustGisApp, t: &Tokens) {
    ui.add_space(6.0);
    ui.separator();
    ui.label(egui::RichText::new(tl!("Web Tile Layers")).font(semibold(12.5)));
    let tiles = app.session.map().tiles.clone();
    if tiles.is_empty() {
        ui.label(egui::RichText::new(tl!("None. Add one by address, or from a basemap plugin.")).weak().small());
    }
    let mut action: Option<(&str, Value)> = None;
    for (i, layer) in tiles.iter().enumerate() {
        ui.horizontal(|ui| {
            let mut vis = layer.visible;
            if ui.checkbox(&mut vis, "").on_hover_text(tl!("Show or hide")).changed() {
                action = Some(("map.setTiles", json!({"index": i, "visible": vis})));
            }
            let name = if layer.overlay { format!("{} · {}", layer.name, tl!("overlay")) } else { layer.name.clone() };
            ui.allocate_ui_with_layout(vec2(160.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_width(160.0);
                ui.add(egui::Label::new(egui::RichText::new(name).color(t.text)).truncate()).on_hover_text(&layer.url);
            });
            let mut o = layer.opacity * 100.0;
            let slider = ui.add(egui::Slider::new(&mut o, 0.0..=100.0).suffix("%").show_value(false)).on_hover_text(tl!("Opacity"));
            if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                action = Some(("map.setTiles", json!({"index": i, "opacity": o / 100.0})));
            }
            if ui.add_enabled(i > 0, egui::Button::new("▲").small()).on_hover_text(tl!("Move up")).clicked() {
                action = Some(("map.setTiles", json!({"index": i, "move": "up"})));
            }
            if ui.add_enabled(i + 1 < tiles.len(), egui::Button::new("▼").small()).on_hover_text(tl!("Move down")).clicked() {
                action = Some(("map.setTiles", json!({"index": i, "move": "down"})));
            }
            if crate::widgets::icon_button(ui, "close", tl!("Remove")).clicked() {
                action = Some(("map.removeTiles", json!({"index": i})));
            }
        });
    }
    if let Some((cmd, p)) = action {
        let _ = app.run(cmd, p);
    }
    ui.horizontal(|ui| {
        if ui.button(tl!("Add Tile Layer…")).on_hover_text(tl!("Add raster tiles, vector tiles or a map style by address.")).clicked() {
            let _ = app.run("ui.dialog", json!({"name": "addTiles"}));
            ui.close();
        }
        if !tiles.is_empty() && ui.button(tl!("Reload")).on_hover_text(tl!("Fetch the tiles and styles of the web layers again.")).clicked()
        {
            let _ = app.run("map.reloadWebLayers", json!({}));
        }
        if !tiles.is_empty() && ui.button(tl!("Remove All")).clicked() {
            let _ = app.run("map.removeTiles", json!({}));
        }
    });
}

fn offline_group(app: &mut RustGisApp, ui: &mut Ui) {
    group(ui, "Offline", |ui| {
        big(
            ui,
            app,
            "export",
            "Download\nMap",
            "Take the data in view offline, to edit and sync back later.",
            "offline.download",
            Value::Null,
            false,
            false,
        );
        col(ui, |ui| {
            small_menu(ui, app, "map", "Offline Areas", "Sync, open or remove a downloaded map.", |ui, app| {
                let areas: Vec<String> = app.session.project.offline_areas.iter().map(|a| a.name.clone()).collect();
                for (label, cmd) in [("Sync", "offline.sync"), ("Open", "offline.open"), ("Remove", "offline.remove")] {
                    submenu_row(ui, "", tl!(label), |ui| {
                        if areas.is_empty() {
                            ui.label(egui::RichText::new(tl!("No offline areas")).weak());
                        }
                        for name in &areas {
                            menu_item(ui, app, "", name, cmd, json!({"name": name}));
                        }
                    });
                }
            });
        });
    });
}

fn labeling_group(app: &mut RustGisApp, ui: &mut Ui) {
    group(ui, "Labeling", |ui| {
        let d = app.session.map().label_display;
        col(ui, |ui| {
            small(
                ui,
                app,
                "label_lock",
                "Lock",
                "Keep the labelled features fixed as the map moves.",
                "labels.lock",
                Value::Null,
                d.locked,
            );
            small(ui, app, "label_pause", "Pause", "Stop drawing labels, for faster navigation.", "labels.pause", Value::Null, d.paused);
            small(
                ui,
                app,
                "label_unplaced",
                "View Unplaced",
                "Show the labels that don't fit, in red.",
                "labels.viewUnplaced",
                Value::Null,
                d.show_unplaced,
            );
        });
        col(ui, |ui| {
            small(
                ui,
                app,
                "label_convert",
                "Convert",
                "Convert the selected layer's labels into an annotation layer.",
                "labels.convert",
                Value::Null,
                false,
            );
        });
    });
}

/// A publication profile's name in the menu: a `tl!` literal each, so the catalogs find it (the
/// engine's labels, which it matches in English); `None` for a profile added without one.
fn publication_label(id: &str) -> Option<&'static str> {
    Some(match id {
        "a4-portrait" => tl!("A4 portrait sheet"),
        "a4-landscape" => tl!("A4 landscape sheet"),
        "a3-portrait" => tl!("A3 portrait sheet"),
        "a3-landscape" => tl!("A3 landscape sheet"),
        "a2-portrait" => tl!("A2 portrait sheet"),
        "a2-landscape" => tl!("A2 landscape sheet"),
        "figure-1col" => tl!("Journal figure, 1 column (90 mm)"),
        "figure-1.5col" => tl!("Journal figure, 1.5 columns (140 mm)"),
        "figure-2col" => tl!("Journal figure, 2 columns (190 mm)"),
        "thesis-full" => tl!("Thesis figure, full page (160 × 225 mm)"),
        "thesis-half" => tl!("Thesis figure, half page (160 × 110 mm)"),
        _ => return None,
    })
}

fn project_group(app: &mut RustGisApp, ui: &mut Ui) {
    group(ui, "Project", |ui| {
        big(ui, app, "new_map", "New\nMap", "Add a new map to the project.", "map.new", Value::Null, false, false);
        big_split_menu(
            ui,
            app,
            "new_layout",
            "New\nLayout",
            "Add a Letter page layout.",
            "layout.new",
            json!({"page": "letter"}),
            false,
            "New Layout options",
            "Choose a page size, publication layout or imported layout.",
            true,
            |ui, app| {
                for (id, label, w, h) in rustgis_engine::map::layout::PAGES {
                    let text = format!("{} ({w:.0} × {h:.0} mm)", tl!(*label));
                    if ui.button(text).clicked() {
                        let _ = app.run("layout.new", json!({"page": id}));
                        ui.close();
                    }
                }
                // A finished sheet or figure of the map view, its texts to fill in on the page.
                ui.menu_button(tl!("Publication Layout"), |ui| {
                    for profile in rustgis_engine::cmd::layout::PUBLICATION_PROFILES {
                        let label = publication_label(profile.id).unwrap_or_else(|| tl!(profile.label));
                        if ui.button(label).clicked() {
                            // A sheet is titled with the map's name; a figure's title is its caption's.
                            let mut p = json!({"profile": profile.id});
                            if profile.sheet {
                                p["title"] = json!(app.session.map().name.clone());
                            }
                            // The view's extent may not be known (an empty map): said, and the tab stays.
                            match app.run("layout.publication", p) {
                                Ok(r) => {
                                    // What the layout couldn't do as asked (a legend clipped, a layer still reading).
                                    let said: Vec<&str> =
                                        r["warnings"].as_array().into_iter().flatten().filter_map(|w| w.as_str()).collect();
                                    if !said.is_empty() {
                                        app.session.notify(said.join("\n"), false);
                                    }
                                }
                                Err(e) => app.session.notify(e, true),
                            }
                            ui.close();
                        }
                    }
                });
                menu_item(ui, app, "open", "Import Layout…", "ui.importLayout", Value::Null);
            },
        );
    });
}

fn report_group(app: &mut RustGisApp, ui: &mut Ui) {
    group(ui, "Report", |ui| {
        big(
            ui,
            app,
            "table",
            "New\nReport",
            "A report listing the selected layer's records, page after page.",
            "ui.newReport",
            Value::Null,
            false,
            false,
        );
        col(ui, |ui| {
            small(
                ui,
                app,
                "open",
                "Import Report…",
                "Add a report file, or a report template for the selected layer.",
                "ui.openItem",
                json!({"kind": "report"}),
                false,
            );
            small(
                ui,
                app,
                "pane",
                "Report Pane",
                "Preview and set up the report.",
                "ui.pane.report",
                Value::Null,
                app.ui.right_open.contains(&RightPane::Report),
            );
        });
        col(ui, |ui| {
            small_menu(
                ui,
                app,
                "new_layout",
                "Report Section",
                "Add pages, layouts, related reports and related records to the report.",
                |ui, app| {
                    menu_item(ui, app, "new_layout", "Supplemental Page", "report.supplementalPage", Value::Null);
                    menu_item(
                        ui,
                        app,
                        "new_layout",
                        "Supplemental Page After",
                        "report.supplementalPage",
                        json!({"title": "Notes", "end": true}),
                    );
                    menu_item(ui, app, "new_layout", "New Template (a layout page)", "report.newTemplate", Value::Null);
                    let layouts: Vec<String> = app.session.project.layouts.iter().map(|l| l.name.clone()).collect();
                    ui.menu_button(tl!("Existing Template"), |ui| {
                        for l in layouts {
                            menu_item(ui, app, "", &l, "report.existingTemplate", json!({"layout": l}));
                        }
                        menu_item(ui, app, "open", "Layout File…", "ui.reportLayoutFile", Value::Null);
                    });
                    let active = rustgis_engine::cmd::report::index(&app.session, &Value::Null).ok();
                    let others: Vec<String> = app
                        .session
                        .project
                        .reports
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| Some(*j) != active)
                        .map(|(_, r)| r.name.clone())
                        .collect();
                    ui.menu_button(tl!("Related Report"), |ui| {
                        if others.is_empty() {
                            ui.label(egui::RichText::new(tl!("No other reports")).weak());
                        }
                        for o in others {
                            menu_item(ui, app, "", &o, "report.relatedReport", json!({"related": o}));
                        }
                    });
                    let rels: Vec<String> =
                        app.session.project.maps.iter().flat_map(|m| m.editing.relationships.iter().map(|r| r.name.clone())).collect();
                    ui.menu_button(tl!("Subreport"), |ui| {
                        if rels.is_empty() {
                            ui.label(egui::RichText::new(tl!("No relationships (Data Design › Relationships)")).weak());
                        }
                        for r in rels {
                            menu_item(ui, app, "", &r, "report.subreport", json!({"relationship": r}));
                        }
                    });
                },
            );
            small_menu(
                ui,
                app,
                "field",
                "Report Data",
                "Fields, statistics, groups, pictures, attachments and charts in the report.",
                |ui, app| {
                    let fields: Vec<String> = rustgis_engine::cmd::report::index(&app.session, &Value::Null)
                        .ok()
                        .and_then(|i| rustgis_engine::cmd::report::layer_of(&app.session, i).ok())
                        .map(|l| l.dataset.fields.iter().map(|f| f.name.clone()).collect())
                        .unwrap_or_default();
                    let field_menu = |ui: &mut Ui, app: &mut RustGisApp, label: &str, id: &str, key: &str| {
                        ui.menu_button(tl!(label), |ui| {
                            for f in &fields {
                                menu_item(ui, app, "", f, id, json!({key: f}));
                            }
                        });
                    };
                    field_menu(ui, app, "Field", "report.field", "field");
                    menu_item(ui, app, "field", "Autofill Fields", "report.autofill", Value::Null);
                    ui.menu_button(tl!("Statistics"), |ui| {
                        for f in &fields {
                            ui.menu_button(f.as_str(), |ui| {
                                for k in ["count", "sum", "mean", "min", "max"] {
                                    menu_item(ui, app, "", k, "report.statistics", json!({"field": f, "kind": k}));
                                }
                            });
                        }
                    });
                    field_menu(ui, app, "Group", "report.group", "field");
                    field_menu(ui, app, "Attachments", "report.attachments", "field");
                    field_menu(ui, app, "Dynamic Picture", "report.dynamicPicture", "field");
                    field_menu(ui, app, "Chart Frame", "report.chart", "category");
                },
            );
            small_menu(ui, app, "print", "Report Output", "Export the report, or save it as a report file or template.", |ui, app| {
                menu_item(ui, app, "print", "Export Report…", "ui.exportReport", Value::Null);
                menu_item(ui, app, "save_as", "Save As Report File…", "ui.saveItem", json!({"kind": "report"}));
                menu_item(ui, app, "save_as", "Save As Report Template…", "ui.saveItem", json!({"kind": "reportTemplate"}));
            });
        });
    });
}

fn presentation_group(app: &mut RustGisApp, ui: &mut Ui) {
    group(ui, "Presentation", |ui| {
        big(
            ui,
            app,
            "web_map",
            "New\nPresentation",
            "Pages of maps, text, images and videos, shown one after another.",
            "ui.newPresentation",
            Value::Null,
            false,
            false,
        );
        col(ui, |ui| {
            small(
                ui,
                app,
                "open",
                "Import Presentation…",
                "Add a presentation file.",
                "ui.openItem",
                json!({"kind": "presentation"}),
                false,
            );
            small(
                ui,
                app,
                "pane",
                "Presentation Pane",
                "Preview, edit and play the presentation.",
                "ui.pane.presentation",
                Value::Null,
                app.ui.right_open.contains(&RightPane::Presentation),
            );
        });
        col(ui, |ui| {
            small_menu(ui, app, "new_layout", "Page", "Add a page to the presentation.", |ui, app| {
                menu_item(ui, app, "web_map", "Map (the current view)", "presentation.mapPage", Value::Null);
                menu_item(ui, app, "new_layout", "Blank", "presentation.blankPage", json!({"title": "Title"}));
                menu_item(ui, app, "picture", "Image…", "ui.presentationPicture", Value::Null);
                menu_item(ui, app, "export_map", "Video…", "ui.presentationVideo", Value::Null);
            });
            small_menu(
                ui,
                app,
                "web_map",
                "Presentation Output",
                "Export the presentation as a slideshow or PDF, or save it as a file.",
                |ui, app| {
                    menu_item(ui, app, "web_map", "Export Presentation…", "ui.exportPresentation", Value::Null);
                    menu_item(ui, app, "save_as", "Save As Presentation File…", "ui.saveItem", json!({"kind": "presentation"}));
                },
            );
        });
    });
}

fn notebook_group(app: &mut RustGisApp, ui: &mut Ui) {
    big(ui, app, "python", "New\nNotebook", "A notebook of text and command cells.", "ui.newNotebook", Value::Null, false, false);
    col(ui, |ui| {
        small(ui, app, "open", "Open Notebook…", "Open a RustGIS or Jupyter notebook.", "ui.openItem", json!({"kind": "notebook"}), false);
        small(
            ui,
            app,
            "pane",
            "Notebook Pane",
            "Edit and run the notebook.",
            "ui.pane.notebook",
            Value::Null,
            app.ui.right_open.contains(&RightPane::Notebook),
        );
        small(
            ui,
            app,
            "python",
            "Script",
            "Run commands from text, one per line.",
            "ui.pane.script",
            Value::Null,
            app.ui.right_open.contains(&RightPane::Script),
        );
    });
}

pub(crate) fn layer_templates_gallery(ui: &mut Ui, app: &mut RustGisApp) {
    for (id, label) in [
        ("points", "Points"),
        ("lines", "Lines"),
        ("polygons", "Polygons"),
        ("places", "Places (name, category, address, rating)"),
        ("routes", "Routes (name, surface, length)"),
        ("areas", "Areas (name, land use, area)"),
        ("observations", "Observations (observer, date, species, count)"),
    ] {
        menu_item(ui, app, "add_data", label, "layer.template", json!({"template": id}));
    }
}

fn styles_group(app: &mut RustGisApp, ui: &mut Ui) {
    group(ui, "Styles", |ui| {
        big_menu(ui, app, "symbology", "Styles", "Saved symbology: apply it to the selected layer, or save the layer's.", |ui, app| {
            let styles: Vec<(String, String)> =
                app.session.project.styles.iter().map(|t| (t.name.clone(), t.geometry.label().to_string())).collect();
            if styles.is_empty() {
                ui.label(egui::RichText::new(tl!("No styles yet")).weak());
            }
            for (n, g) in styles {
                menu_item(ui, app, "symbology", &format!("{n} ({})", tl!(&g)), "style.apply", json!({"name": n}));
            }
            ui.separator();
            menu_item(ui, app, "symbology", "New Style From Layer", "style.new", Value::Null);
            menu_item(ui, app, "open", "Add Style File…", "ui.openItem", json!({"kind": "styles"}));
            menu_item(ui, app, "open", "Import Styles From…", "ui.openItem", json!({"kind": "importStyles"}));
            menu_item(ui, app, "open", "Add Color Ramp File…", "ui.openItem", json!({"kind": "ramps"}));
        });
    });
}

fn graphics_group(app: &mut RustGisApp, ui: &mut Ui) {
    group(ui, "Graphics", |ui| {
        for tools in [
            &[("g_point", "Point", "graphicPoint"), ("g_line", "Line", "graphicLine"), ("g_curve", "Curve", "graphicCurve")][..],
            &[("g_freehand", "Freehand", "graphicFreehand"), ("picture", "Picture", "graphicPicture")][..],
        ] {
            col(ui, |ui| {
                for (icon, label, code) in tools {
                    if small(
                        ui,
                        app,
                        icon,
                        label,
                        "Draw a graphic on the map.",
                        "tool.set",
                        json!({"tool": code}),
                        app.session.tool.code() == *code,
                    )
                    .clicked()
                    {
                        app.map.cancel_sketch();
                    }
                }
                if tools.len() == 2 {
                    small(
                        ui,
                        app,
                        "keep_tool",
                        "Keep Tool",
                        "Keep the drawing tool active after each graphic.",
                        "graphics.keepTool",
                        Value::Null,
                        app.session.keep_tool,
                    );
                }
            });
        }
        let family = ["graphicPolygon", "graphicLasso", "graphicRectangle", "graphicCircle", "graphicEllipse"];
        let (primary, _) = big_split_menu(
            ui,
            app,
            "g_polygon",
            "Polygon",
            "Draw a polygon graphic.",
            "tool.set",
            json!({"tool": "graphicPolygon"}),
            family.contains(&app.session.tool.code()),
            "Polygon options",
            "Choose a polygon drawing tool.",
            true,
            |ui, app| {
                for (label, code) in [
                    ("Lasso", "graphicLasso"),
                    ("Rectangle", "graphicRectangle"),
                    ("Circle", "graphicCircle"),
                    ("Ellipse", "graphicEllipse"),
                ] {
                    if ui.button(tl!(label)).clicked() {
                        app.map.cancel_sketch();
                        let _ = app.run("tool.set", json!({"tool": code}));
                        ui.close();
                    }
                }
            },
        );
        if primary.clicked() {
            app.map.cancel_sketch();
        }
        let (primary, _) = big_split_menu(
            ui,
            app,
            "g_text",
            "Text",
            "Click the map to place text.",
            "tool.set",
            json!({"tool": "graphicText"}),
            app.session.tool.code() == "graphicText",
            "Text options",
            "Place a live map value as text.",
            true,
            |ui, app| {
                for &(token, what) in rustgis_engine::map::graphics::TOKENS {
                    if ui.button(format!("{}  {token}", tl!(what))).clicked() {
                        app.map.cancel_sketch();
                        let _ = app.run("ui.dynamicText", json!({"text": token}));
                        ui.close();
                    }
                }
            },
        );
        if primary.clicked() {
            app.map.cancel_sketch();
        }
    });
}

/// Commands the Quick Access Toolbar can show: (id, label). Their icons are in [`quick_icon`].
pub const QUICK: &[(&str, &str)] = &[
    ("ui.saveAs", "Save As"),
    ("ui.exportMap", "Export Map"),
    ("ui.printMap", "Print Map"),
    ("table.open", "Attribute Table"),
    ("map.zoomFull", "Full Extent"),
    ("select.clear", "Clear Selection"),
    ("ui.pane.geoprocessing", "Tools Pane"),
    ("ui.commandSearch", "Command Search"),
];

/// The icon of a Quick Access command.
pub fn quick_icon(id: &str) -> &'static str {
    match id {
        "ui.saveAs" => "save_as",
        "ui.exportMap" => "export_map",
        "ui.printMap" => "print",
        "table.open" => "table",
        "map.zoomFull" => "zoom_full",
        "select.clear" => "clear_sel",
        "ui.pane.geoprocessing" => "toolbox",
        _ => "search",
    }
}

/// Analysis gallery entries: (tool id, icon, caption).
const GALLERY: &[(&str, &str, &str)] = &[
    ("buffer", "buffer", "Buffer"),
    ("clip", "clip", "Clip"),
    ("erase", "erase", "Erase"),
    ("dissolve", "dissolve", "Dissolve"),
    ("countPoints", "count", "Summarize\nWithin"),
    ("convexHull", "hull", "Bounding\nGeometry"),
    ("featureToPoint", "centroid", "Feature\nTo Point"),
    ("simplify", "simplify", "Simplify"),
    ("calculateGeometry", "calc_geom", "Calculate\nGeometry"),
    ("summaryStatistics", "stats", "Summary\nStatistics"),
    ("exportFeatures", "export_layer", "Export\nFeatures"),
    ("idwInterpolation", "relief", "IDW\nInterpolation"),
];

fn analysis_groups(app: &mut RustGisApp, ui: &mut Ui) {
    group(ui, "Geoprocessing", |ui| {
        big(
            ui,
            app,
            "toolbox",
            "Tools",
            "Find and run geoprocessing tools.",
            "ui.pane.geoprocessing",
            Value::Null,
            app.ui.right_open.contains(&RightPane::Geoprocessing),
            false,
        );
        big(
            ui,
            app,
            "modelbuilder",
            "Model\nBuilder",
            "Draw a model: chain tools from input layers to the results to keep.",
            "ui.modelBuilder",
            Value::Null,
            app.mb.shown(),
            false,
        );
        col(ui, |ui| {
            small(
                ui,
                app,
                "history",
                "History",
                "See the tools you've run.",
                "ui.pane.history",
                Value::Null,
                app.ui.right_open.contains(&RightPane::History),
            );
            small(
                ui,
                app,
                "environments",
                "Environments",
                "Limit where tools work, and the cell size of new rasters.",
                "ui.pane.workflows",
                json!({"section": "environments"}),
                app.ui.right_open.contains(&RightPane::Workflows) && app.workflow.section == "environments",
            );
            small(
                ui,
                app,
                "def_query",
                "SQL",
                "Query files, web addresses and databases with DuckDB Spatial; add results to the map.",
                "ui.pane.sql",
                Value::Null,
                app.ui.right_open.contains(&RightPane::Sql),
            );
        });
    });
    group(ui, "Automate", |ui| {
        big(
            ui,
            app,
            "run",
            "Tasks",
            "Step through a task.",
            "ui.pane.tasks",
            Value::Null,
            app.ui.right_open.contains(&RightPane::Tasks),
            false,
        );
        col(ui, |ui| {
            small_menu(ui, app, "run", "Task", "Tasks: guided steps that each run a command.", |ui, app| {
                let record = if app.ws.recording.is_some() { "Stop Recording" } else { "Record Task" };
                menu_item(ui, app, "edit", record, "ui.taskRecord", json!({}));
                menu_item(ui, app, "history", "New Task From History", "task.new", json!({"name": "Workflow", "fromHistory": true}));
                menu_item(ui, app, "open", "Import Task…", "ui.openItem", json!({"kind": "task"}));
                ui.menu_button(tl!("Run"), |ui| {
                    let tasks: Vec<String> = app.session.project.tasks.iter().map(|t| t.name.clone()).collect();
                    for task in tasks {
                        menu_item(ui, app, "run", &task, "task.run", json!({"task": task}));
                    }
                });
            });
        });
        notebook_group(app, ui);
        big(
            ui,
            app,
            "assistant",
            "AI\nAssistant",
            "Ask about your data, or tell the assistant what to do with the map.",
            "ui.pane.assistant",
            Value::Null,
            app.ui.right_open.contains(&RightPane::Assistant),
            false,
        );
    });
    group(ui, "Workflows", |ui| {
        big_menu(
            ui,
            app,
            "geoprocessing",
            "Workflows",
            "Guided analysis: suitability, visibility, networks, interpolation and more.",
            |ui, app| {
                for (id, title) in crate::analysispane::SECTIONS.iter().filter(|s| {
                    matches!(
                        s.0,
                        "suitability"
                            | "zonalStats"
                            | "visibility"
                            | "lineOfSight"
                            | "interpolate"
                            | "network"
                            | "neighborhood"
                            | "simulation"
                            | "profile"
                            | "explore"
                            | "similar"
                            | "interop"
                    )
                }) {
                    menu_item(ui, app, "geoprocessing", title, "ui.pane.workflows", json!({"section": id}));
                }
            },
        );
    });
    group(ui, "Tools", |ui| {
        let t = Tokens::get(ui.ctx());
        // A framed gallery like a desktop GIS analysis gallery.
        let r = ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
            ui.spacing_mut().item_spacing = vec2(1.0, 0.0);
            for (id, icon, label) in GALLERY {
                let tool = rustgis_engine::analysis::tool(id);
                let desc = tool.map(|t| t.summary).unwrap_or("");
                big(ui, app, icon, label, desc, "ui.gp.open", json!({"tool": id}), false, false);
            }
        });
        ui.painter().rect_stroke(r.response.rect.expand(1.0), CornerRadius::same(6), Stroke::new(1.0, t.border), egui::StrokeKind::Outside);
    });
}

fn view_tab(app: &mut RustGisApp, ui: &mut Ui) {
    group(ui, "Map", |ui| {
        let on = app.session.map().projection.is_globe();
        big(
            ui,
            app,
            "globe_view",
            "Globe",
            "Show the map as a 3D globe, or as a flat map again.",
            "map.projection",
            Value::Null,
            on,
            false,
        );
        let current = app.session.map().projection.code();
        let local = current.starts_with("local:");
        big_menu(ui, app, "basemap", "Projection", "Choose how the flat map lays out the Earth.", move |ui, app| {
            let mut items: Vec<(String, String)> = vec![("mercator".into(), "Web Mercator".into())];
            items.extend(rustgis_engine::map::projection::PRESETS.iter().map(|(id, label, _)| (id.to_string(), label.to_string())));
            for (id, label) in items {
                if ui.add(egui::Button::selectable(is_projection(&current, &id), tl!(label.as_str()))).clicked() {
                    if app.run("map.projection", json!({"type": id})).is_ok() {
                        let _ = app.run("map.zoomFull", Value::Null);
                    }
                    ui.close();
                }
            }
            ui.separator();
            if ui.button(tl!("Coordinate System…")).clicked() {
                let _ = app.run("ui.dialog", json!({"name": "coordinateSystem"}));
                ui.close();
            }
        });
        col(ui, |ui| {
            small(ui, app, "globe_view", "Global", "A globe scene.", "map.viewMode", json!({"mode": "global"}), on);
            if rustgis_engine::map::projection::local_views_supported() {
                small(
                    ui,
                    app,
                    "map",
                    "Local",
                    "A flat scene centred on the view (azimuthal equidistant).",
                    "map.viewMode",
                    json!({"mode": "local"}),
                    local,
                );
            }
            let g = app.session.map().graticule;
            small(ui, app, "graticule", "Graticule", "Show latitude/longitude lines on the basemap.", "map.graticule", Value::Null, g);
        });
    });
    let v = app.session.map().view.clone();
    group(ui, "Navigation", |ui| {
        col(ui, |ui| {
            small(ui, app, "zoom_world", "World", "Zoom to the whole world.", "map.zoomWorld", Value::Null, false);
            small(
                ui,
                app,
                "explore",
                "Navigator",
                "On-screen compass, zoom slider and pan pad.",
                "map.navigator",
                Value::Null,
                v.navigator,
            );
            small(
                ui,
                app,
                "goto_xy",
                "Camera",
                "See and set the camera's position and scale.",
                "ui.dialog",
                json!({"name": "camera"}),
                false,
            );
        });
        col(ui, |ui| {
            small(
                ui,
                app,
                "search",
                "Magnifier",
                "A magnifying lens that follows the pointer.",
                "ui.magnifier",
                Value::Null,
                app.map.magnifier,
            );
            let fp = v.first_person;
            small_menu(ui, app, "explore", "Navigation Mode", "Move over the map, or walk and fly through it.", move |ui, app| {
                for (label, mode, on) in [("Planar", "planar", !fp), ("First-Person", "firstPerson", fp)] {
                    if ui.radio(on, tl!(label)).clicked() {
                        let _ = app.run("map.navigation", json!({"mode": mode}));
                        ui.close();
                    }
                }
            });
            let popup = app.session.map().popup_mode;
            small_menu(ui, app, "identify", "Identify", "Which layers a click on the map identifies.", move |ui, app| {
                for m in rustgis_engine::map::PopupMode::ALL {
                    if ui.radio(popup == m, tl!(m.label())).clicked() {
                        let _ = app.run("map.popupMode", json!({"mode": m.code()}));
                        ui.close();
                    }
                }
            });
        });
    });
    group(ui, "Effects", |ui| {
        let fx = v.effects.clone();
        big_menu(
            ui,
            app,
            "brightness",
            "Effects",
            "Illumination, weather, visual and camera effects, and postprocessing for the map.",
            move |ui, app| {
                ui.menu_button(tl!("Illumination"), |ui| {
                    if ui.radio(fx.sun_time.is_none(), tl!("Off")).clicked() {
                        let _ = app.run("map.effects", json!({"sunTime": null}));
                        ui.close();
                    }
                    if ui.button(tl!("Now")).clicked() {
                        let _ = app.run("map.effects", json!({"sunTime": "now"}));
                        ui.close();
                    }
                    let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
                    let day = (now / 86_400.0).floor() * 86_400.0;
                    for (label, h) in [("06:00 UTC", 6.0), ("12:00 UTC", 12.0), ("18:00 UTC", 18.0), ("00:00 UTC", 0.0)] {
                        if ui.button(label).clicked() {
                            let _ = app.run("map.effects", json!({"sunTime": day + h * 3600.0}));
                            ui.close();
                        }
                    }
                });
                ui.menu_button(tl!("Weather"), |ui| {
                    for (label, code, w) in [
                        ("None", "none", Weather::None),
                        ("Haze", "haze", Weather::Haze),
                        ("Fog", "fog", Weather::Fog),
                        ("Rain", "rain", Weather::Rain),
                        ("Snow", "snow", Weather::Snow),
                    ] {
                        if ui.radio(fx.weather == w, tl!(label)).clicked() {
                            let _ = app.run("map.effects", json!({"weather": code}));
                            ui.close();
                        }
                    }
                });
                ui.menu_button(tl!("Visual Effect"), |ui| {
                    for (label, key, on) in
                        [("Vignette", "vignette", fx.vignette), ("Edges", "edges", fx.edges), ("Stars", "stars", fx.stars)]
                    {
                        if ui.checkbox(&mut on.clone(), tl!(label)).clicked() {
                            let _ = app.run("map.effects", json!({key: !on}));
                        }
                    }
                });
                ui.menu_button(tl!("Camera Effect"), |ui| {
                    for (label, key, on) in [("Tilt-shift", "tiltShift", fx.tilt_shift), ("Film grain", "grain", fx.grain)] {
                        if ui.checkbox(&mut on.clone(), tl!(label)).clicked() {
                            let _ = app.run("map.effects", json!({key: !on}));
                        }
                    }
                });
                ui.menu_button(tl!("Postprocessing"), |ui| {
                    for (label, key, mut val, lo) in [
                        ("Brightness", "brightness", fx.brightness, -100.0),
                        ("Contrast", "contrast", fx.contrast, -100.0),
                        ("Saturation", "saturation", fx.saturation, -100.0),
                        ("Sepia", "sepia", fx.sepia, 0.0),
                    ] {
                        ui.horizontal(|ui| {
                            let input_label = ui.label(tl!(label));
                            let r = ui.add(egui::Slider::new(&mut val, lo..=100.0)).labelled_by(input_label.id);
                            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                                let _ = app.run("map.effects", json!({key: val}));
                            }
                        });
                    }
                    if ui.button(tl!("Reset Effects")).clicked() {
                        let _ = app.run("map.effects", json!({"reset": true}));
                        ui.close();
                    }
                });
            },
        );
        col(ui, |ui| {
            let cv = v.color_vision;
            small_menu(ui, app, "eye", "Color Vision", "See the map as people with colour vision deficiencies do.", move |ui, app| {
                for c in rustgis_engine::map::view::ColorVision::ALL {
                    if ui.radio(cv == c, tl!(c.label())).clicked() {
                        let _ = app.run("map.colorVision", json!({"mode": c.code()}));
                        ui.close();
                    }
                }
            });
            let (wireframe, fast, clip) = (v.wireframe, v.fast, v.clip_distance);
            small_menu(ui, app, "polygon_tool", "Scene", "Wireframe drawing, faster display and view clipping.", move |ui, app| {
                if ui
                    .add(egui::Button::selectable(wireframe, tl!("Wireframe")))
                    .on_hover_text(tl!("Draw polygons as outlines only."))
                    .clicked()
                {
                    let _ = app.run("map.drawingMode", json!({"mode": if wireframe { "standard" } else { "wireframe" }}));
                    ui.close();
                }
                if ui
                    .add(egui::Button::selectable(fast, tl!("Speed Display")))
                    .on_hover_text(tl!("Faster drawing: no antialiasing."))
                    .clicked()
                {
                    let _ = app.run("map.displayType", json!({"mode": if fast { "quality" } else { "speed" }}));
                    ui.close();
                }
                ui.menu_button(tl!("View Clipping"), |ui| {
                    if ui.radio(clip.is_none(), tl!("None")).clicked() {
                        let _ = app.run("map.clipping", json!({"mode": "none"}));
                        ui.close();
                    }
                    for km in [100.0, 500.0, 1000.0, 3000.0] {
                        if ui
                            .radio(clip == Some(km * 1000.0), crate::measure_units::distance(km * 1000.0, app.ui.measurement_imperial))
                            .clicked()
                        {
                            let _ = app.run("map.clipping", json!({"mode": "distance", "distance": km * 1000.0}));
                            ui.close();
                        }
                    }
                });
            });
        });
    });
    group(ui, "Windows", |ui| {
        big(
            ui,
            app,
            "contents",
            "Layers",
            "Show or hide the Layers pane.",
            "ui.pane.contents",
            Value::Null,
            app.ui.left == Some(crate::LeftPane::Contents),
            false,
        );
        big(
            ui,
            app,
            "catalog",
            "Browser",
            "Show or hide the Browser pane.",
            "ui.pane.catalog",
            Value::Null,
            app.ui.left == Some(crate::LeftPane::Catalog),
            false,
        );
        big(
            ui,
            app,
            "web_map",
            "Data\nSources",
            "Show or hide the Data Sources pane: favourites, recent data, saved services, databases and STAC APIs, and a library of public sources.",
            "ui.pane.sources",
            Value::Null,
            app.ui.left == Some(crate::LeftPane::Sources),
            false,
        );
        col(ui, |ui| {
            small(
                ui,
                app,
                "identify",
                "Details",
                "Details of the selected layer.",
                "ui.pane.details",
                Value::Null,
                app.ui.right_open.contains(&RightPane::Details),
            );
        });
        let left = |p: crate::LeftPane| app.ui.left == Some(p);
        let right = |p: RightPane| app.ui.right_open.contains(&p);
        let mut panes: Vec<(String, &'static str, bool)> = [
            ("Layers", "ui.pane.contents", left(crate::LeftPane::Contents)),
            ("Browser", "ui.pane.catalog", left(crate::LeftPane::Catalog)),
            ("Data Sources", "ui.pane.sources", left(crate::LeftPane::Sources)),
            ("Tools", "ui.pane.geoprocessing", right(RightPane::Geoprocessing)),
            ("Symbology", "ui.pane.symbology", right(RightPane::Symbology)),
            ("Labeling", "ui.pane.labeling", right(RightPane::Labeling)),
            ("History", "ui.pane.history", right(RightPane::History)),
            ("Details", "ui.pane.details", right(RightPane::Details)),
            ("Bookmarks", "ui.pane.bookmarks", right(RightPane::Bookmarks)),
            ("AI Assistant", "ui.pane.assistant", right(RightPane::Assistant)),
            ("Tasks", "ui.pane.tasks", right(RightPane::Tasks)),
            ("Editing", "ui.pane.editing", right(RightPane::Editing)),
            ("Data Design", "ui.pane.design", right(RightPane::Design)),
            ("Workflows", "ui.pane.workflows", right(RightPane::Workflows)),
            ("Script", "ui.pane.script", right(RightPane::Script)),
            ("SQL", "ui.pane.sql", right(RightPane::Sql)),
            ("Notebook", "ui.pane.notebook", right(RightPane::Notebook)),
            ("Workflow Manager", "ui.pane.jobs", right(RightPane::Jobs)),
            ("Profile", "ui.pane.profile", right(RightPane::Profile)),
            ("Time Series", "ui.pane.timeSeries", right(RightPane::TimeSeries)),
            ("Diagnostic Monitor", "ui.pane.diagnostics", right(RightPane::Diagnostics)),
            ("Infographics", "ui.pane.infographics", right(RightPane::Infographics)),
            ("Directions", "ui.pane.directions", right(RightPane::Directions)),
            ("Web Data", "ui.pane.webData", right(RightPane::WebData)),
            ("Report", "ui.pane.report", right(RightPane::Report)),
            ("Presentation", "ui.pane.presentation", right(RightPane::Presentation)),
            ("Format", "ui.pane.format", right(RightPane::Format)),
            ("Shared Items", "ui.pane.shares", right(RightPane::Shares)),
        ]
        .into_iter()
        // A Zarr series is read by the desktop app (the web app can't read one yet).
        .filter(|p| !(cfg!(target_arch = "wasm32") && p.1 == "ui.pane.timeSeries"))
        .map(|(label, id, on)| (tl!(label).to_string(), id, on))
        .collect();
        // The plugin panel opens without an id only once a plugin's panel has been opened.
        if app.ui.plugin_panel.plugin.is_some() {
            panes.push((crate::pluginpanel::title(app), "ui.pane.plugin", right(RightPane::Plugin)));
        }
        big_menu(ui, app, "pane", "Panes", "Show or hide any pane.", move |ui, app| {
            for (label, id, on) in panes {
                if ui.add(egui::Button::selectable(on, label)).clicked() {
                    let _ = app.run(id, Value::Null);
                    ui.close();
                }
            }
        });
    });
    labeling_group(app, ui);
    group(ui, "Link", |ui| {
        col(ui, |ui| {
            let split = crate::mapview::split_map(app).is_some();
            small(ui, app, "pane_right", "Side by Side", "Show a second map beside this one.", "ui.splitView", Value::Null, split);
            small(ui, app, "zoom_full", "Link Views", "Maps move together.", "view.linkViews", Value::Null, app.session.project.link_views);
            small(
                ui,
                app,
                "goto_xy",
                "Link Cursors",
                "Show the pointer's place on the other map.",
                "ui.linkCursors",
                Value::Null,
                app.ws.link_cursors,
            );
        });
    });
    group(ui, "Time", |ui| {
        big(
            ui,
            app,
            "time_slider",
            "Time\nSlider",
            "Show layers through time: only the features whose date falls in the slider's window, and dated imagery for its date.",
            "ui.timeSlider",
            Value::Null,
            crate::timebar::open(app),
            false,
        );
        let (n, playing) = (app.session.map().animation.len(), app.ws.playing.is_some());
        big_menu(ui, app, "run", "Animation", "Keyframes of the view, played or exported.", move |ui, app| {
            menu_item(ui, app, "bookmark", &crate::i18n::fmt("Add Keyframe ({n})", &[("n", &n.to_string())]), "animation.add", Value::Null);
            menu_item(ui, app, "delete", "Remove", "animation.remove", Value::Null);
            if ui.add(egui::Button::selectable(playing, tl!("Play"))).clicked() {
                let _ = app.run("ui.animationPlay", Value::Null);
                ui.close();
            }
            menu_item(ui, app, "export", "Export…", "ui.exportAnimation", Value::Null);
        });
        col(ui, |ui| {
            small(
                ui,
                app,
                "measure",
                "Profile",
                "Elevation profiles along lines, cross sections and more.",
                "ui.pane.profile",
                Value::Null,
                app.ui.right_open.contains(&RightPane::Profile),
            );
            // A Zarr series is read by the desktop app (the web app can't read one yet).
            if !cfg!(target_arch = "wasm32") {
                small(
                    ui,
                    app,
                    "measure",
                    "Time Series",
                    "A Zarr variable's values through time at points clicked on the map.",
                    "ui.pane.timeSeries",
                    Value::Null,
                    app.ui.right_open.contains(&RightPane::TimeSeries),
                );
            }
        });
    });
    group(ui, "Workspace", |ui| {
        col(ui, |ui| {
            small(
                ui,
                app,
                "catalog",
                "Browser View",
                "The project's items in place of the map.",
                "ui.catalogView",
                Value::Null,
                app.ws.catalog_view,
            );
            small(
                ui,
                app,
                "history",
                "Workflow Manager",
                "Track jobs and their status.",
                "ui.pane.jobs",
                Value::Null,
                app.ui.right_open.contains(&RightPane::Jobs),
            );
            let sets: Vec<String> = app.ui.pane_sets.iter().map(|x| x.0.clone()).collect();
            small_menu(ui, app, "pane", "Pane Sets", "Save the panes you have open, and switch back to them.", move |ui, app| {
                for n in &sets {
                    if ui.button(n.as_str()).clicked() {
                        let _ = app.run("ui.paneSet", json!({"load": n}));
                        ui.close();
                    }
                }
                ui.separator();
                for n in ["Mapping", "Editing", "Analysis"] {
                    if ui.button(crate::i18n::fmt("Save as {name}", &[("name", tl_raw!(n))])).clicked() {
                        let _ = app.run("ui.paneSet", json!({"save": tl!(n)}));
                        ui.close();
                    }
                }
            });
        });
    });
}

/// The Data tab: bringing data in, tables, fields, queries, relationships and export.
fn data_tab(app: &mut RustGisApp, ui: &mut Ui) {
    if active_layer(app).is_some_and(|l| l.is_raster()) {
        raster_data_groups(app, ui);
    } else {
        feature_data_groups(app, ui);
    }
    offline_group(app, ui);
}

/// The Style tab: the drawing of the layer selected in Layers (its symbology, labels and
/// effects), or a hint when none is.
fn style_tab(app: &mut RustGisApp, ui: &mut Ui) {
    match active_layer(app) {
        Some(l) if l.is_raster() => raster_style_groups(app, ui),
        Some(l) if l.geometry_type() != GeometryType::None => {
            feature_style_groups(app, ui);
            label_groups(app, ui);
        }
        _ => group(ui, "Layer", |ui| {
            let t = Tokens::get(ui.ctx());
            ui.add_sized(
                vec2(220.0, CONTENT_H),
                egui::Label::new(egui::RichText::new(tl!("Select a layer in Layers to style it.")).color(t.text_dim)).wrap(),
            );
        }),
    }
    styles_group(app, ui);
}

/// The Analyze tab: geoprocessing, workflows, imagery and notebooks.
fn analyze_tab(app: &mut RustGisApp, ui: &mut Ui) {
    analysis_groups(app, ui);
    imagery_groups(app, ui);
}

/// The Publish tab: map export, reports, presentations and sharing. Layouts have their own tab.
fn publish_tab(app: &mut RustGisApp, ui: &mut Ui) {
    group(ui, "Export", |ui| {
        big(
            ui,
            app,
            "export_map",
            "Export\nMap",
            "Export the map view as a PNG image (with labels and a scale bar).",
            "ui.exportMap",
            Value::Null,
            false,
            false,
        );
        col(ui, |ui| {
            small(
                ui,
                app,
                "print",
                "Print Map",
                "Print the map view (through the system's PDF viewer).",
                "ui.printMap",
                Value::Null,
                false,
            );
            small(
                ui,
                app,
                "copy",
                "Capture To Clipboard",
                "Copy an image of the map to the clipboard.",
                "ui.captureMap",
                Value::Null,
                false,
            );
        });
    });
    report_group(app, ui);
    presentation_group(app, ui);
    share_groups(app, ui);
}

/// The layer selected in Layers.
fn active_layer(app: &RustGisApp) -> Option<&rustgis_engine::map::Layer> {
    app.session.active_layer.and_then(|id| app.session.map().layer(id))
}

fn edit_tab(app: &mut RustGisApp, ui: &mut Ui) {
    let editing = app.session.map().editing.clone();
    group(ui, "Manage Edits", |ui| {
        big(ui, app, "edit", "Edit", "Turn editing on or off for the map.", "edit.enable", Value::Null, editing.settings.enabled, false);
        big(ui, app, "save", "Save", "Save the project with your edits.", "ui.save", Value::Null, false, false);
        col(ui, |ui| {
            small(ui, app, "undo", "Undo", "Undo the last change.", "edit.undo", Value::Null, false);
            small(ui, app, "redo", "Redo", "Redo the change you undid.", "edit.redo", Value::Null, false);
            small(
                ui,
                app,
                "pane_right",
                "Editing Pane",
                "Templates, topology and editing settings.",
                "ui.pane.editing",
                Value::Null,
                app.ui.right == Some(RightPane::Editing),
            );
        });
        col(ui, |ui| {
            small(ui, app, "polygon_tool", "Validate Topology", "Validate topology rules.", "topology.validate", Value::Null, false);
        });
    });
    group(ui, "Selection", |ui| {
        tool_btn(ui, app, "select", "Select", "Click or drag a box to select features. Shift adds, Ctrl removes.", Tool::Select);
        col(ui, |ui| {
            small(ui, app, "clear_sel", "Clear", "Clear the selection in every layer.", "select.clear", Value::Null, false);
        });
    });
    group(ui, "Options", |ui| {
        big(
            ui,
            app,
            "goto_xy",
            "Snapping",
            "Snap new vertices to nearby vertices, edges and line ends.",
            "edit.snapping",
            Value::Null,
            editing.snapping.enabled,
            false,
        );
        col(ui, |ui| {
            small_menu(ui, app, "relief", "Elevation", "Where new features get their elevation.", |ui, app| {
                menu_item(ui, app, "", "None", "edit.elevation", json!({"mode": "none"}));
                menu_item(ui, app, "relief", "Constant…", "ui.pane.editing", Value::Null);
                let rasters: Vec<String> = app.session.map().layers.iter().filter(|l| l.is_raster()).map(|l| l.name.clone()).collect();
                for name in rasters {
                    menu_item(
                        ui,
                        app,
                        "relief",
                        &crate::i18n::fmt("Surface: {layer}", &[("layer", &name)]),
                        "edit.elevation",
                        json!({"mode": "surface", "surface": name}),
                    );
                }
            });
        });
    });
    group(ui, "Features", |ui| {
        let gt = active_layer(app).map(|l| l.geometry_type());
        let (tool, icon) = match gt {
            Some(GeometryType::Polyline) => (Tool::CreateLine, "line_tool"),
            Some(GeometryType::Polygon) => (Tool::CreatePolygon, "polygon_tool"),
            _ => (Tool::CreatePoint, "point_tool"),
        };
        if big(
            ui,
            app,
            icon,
            "Create",
            "Draw new features in the selected layer.",
            "tool.set",
            json!({"tool": tool.code()}),
            app.session.tool.creates(),
            false,
        )
        .clicked()
        {
            app.map.cancel_sketch();
        }
        tool_btn(
            ui,
            app,
            "vertices",
            "Edit\nVertices",
            "Drag vertices, click an edge to add one, Delete to remove one.",
            Tool::EditVertices,
        );
        big(ui, app, "delete", "Delete", "Delete the selected features.", "edit.delete", Value::Null, false, false);
        big_menu(ui, app, "tools", "Editor\nTools", "Merge, transform or split selected features.", |ui, app| {
            for (label, id, params) in [
                ("Merge", "edit.merge", Value::Null),
                ("Explode", "edit.explode", Value::Null),
                ("Flip Line", "edit.flip", Value::Null),
                ("Rotate 90°", "edit.rotate", json!({"angle": 90})),
                ("Scale ×2", "edit.scale", json!({"factor": 2})),
                ("Scale ×½", "edit.scale", json!({"factor": 0.5})),
            ] {
                menu_item(ui, app, "calc_geom", label, id, params);
            }
            if ui.button(tl!("Split (click the line)")).clicked() {
                app.map.split_mode = true;
                ui.close();
            }
        });
    });
    graphics_group(app, ui);
    clipboard_group(app, ui);
}

fn share_groups(app: &mut RustGisApp, ui: &mut Ui) {
    group(ui, "Share", |ui| {
        big_split_menu(
            ui,
            app,
            "package",
            "Project\nPackage",
            "Bundle the project and its data into one file to share.",
            "ui.saveItem",
            json!({"kind": "projectPackage"}),
            false,
            "Project Package options",
            "Choose the kind of package to save.",
            true,
            |ui, app| {
                for (icon, label, kind) in [
                    ("package", "Project", "projectPackage"),
                    ("map", "Map", "mapPackage"),
                    ("map", "Mobile Map", "mobilePackage"),
                    ("history", "Results", "gpPackage"),
                    ("modelbuilder", "Deep Learning Model", "dlPackage"),
                ] {
                    menu_item(ui, app, icon, label, "ui.saveItem", json!({"kind": kind}));
                }
            },
        );
        big_split_menu(
            ui,
            app,
            "web_map",
            "Web\nMap",
            "Share the map as a web page that needs no server.",
            "ui.share",
            json!({"kind": "webMap"}),
            false,
            "Web Map options",
            "Choose the kind of web item to share.",
            true,
            |ui, app| {
                for (icon, label, kind) in [
                    ("web_map", "Web Map", "webMap"),
                    ("export_layer", "Web Layer", "webLayer"),
                    ("symbology", "Web Style", "webStyle"),
                    ("search", "Locator", "locator"),
                    ("package", "Geodata Service", "geodataService"),
                    ("modelbuilder", "Web Tool", "webTool"),
                    ("globe_view", "Web Scene", "webScene"),
                    ("graphics_layer", "Web Link Chart", "linkChart"),
                ] {
                    menu_item(ui, app, icon, label, "ui.share", json!({"kind": kind}));
                }
            },
        );
        col(ui, |ui| {
            small(
                ui,
                app,
                "history",
                "Share Jobs",
                "Shares made, with Manage tools to update them.",
                "ui.pane.shares",
                Value::Null,
                app.ui.right_open.contains(&RightPane::Shares),
            );
        });
    });
    group(ui, "Save As", |ui| {
        big_split_menu(
            ui,
            app,
            "save_as",
            "Save As",
            "Save the project under a new name.",
            "ui.saveAs",
            Value::Null,
            false,
            "Save As options",
            "Save a project or an item file.",
            true,
            |ui, app| {
                menu_item(ui, app, "save_as", "Project…", "ui.saveAs", Value::Null);
                menu_item(ui, app, "new_layout", "Layout File", "ui.saveLayout", Value::Null);
                for (icon, label, kind) in [
                    ("map", "Map File", "map"),
                    ("add_data", "Layer File", "layer"),
                    ("symbology", "Style Items", "styles"),
                    ("symbology", "Color Ramps", "ramps"),
                    ("save_as", "Project Template", "template"),
                    ("run", "Task Item", "task"),
                ] {
                    menu_item(ui, app, icon, label, "ui.saveItem", json!({"kind": kind}));
                }
            },
        );
    });
}

/// The selected layout element if it's a map frame, else the layout's first frame.
fn layout_frame(app: &RustGisApp) -> Option<u32> {
    let l = app.session.active_layout.and_then(|i| app.session.project.layouts.get(i))?;
    app.layout
        .selected
        .filter(|id| matches!(l.element(*id).map(|e| &e.kind), Some(rustgis_engine::map::layout::ElementKind::MapFrame { .. })))
        .or_else(|| l.first_frame())
}

fn layout_groups(app: &mut RustGisApp, ui: &mut Ui) {
    use rustgis_engine::map::layout::ElementKind;
    let Some(li) = app.session.active_layout else { return };
    let Some(layout) = app.session.project.layouts.get(li).cloned() else { return };
    let frame = layout_frame(app);
    let add = |app: &mut RustGisApp, mut p: Value| {
        p["layout"] = json!(li);
        if let Some(f) = frame {
            p["frame"] = json!(f);
        }
        match app.run("layout.addElement", p) {
            Ok(v) => app.layout.selected = v["element"].as_u64().map(|v| v as u32),
            // A locked frame takes no grid or extent indicator: say so rather than nothing (#536).
            Err(e) => app.session.notify(e, true),
        }
    };
    group(ui, "Page", |ui| {
        big_menu(ui, app, "new_layout", "Page\nSize", "Change the page size and orientation.", |ui, app| {
            for (id, label, w, h) in rustgis_engine::map::layout::PAGES {
                let landscape = layout.width > layout.height;
                let (pw, ph) = if landscape { (h.max(*w), h.min(*w)) } else { (*w, *h) };
                let on = (layout.width - pw).abs() < 0.5 && (layout.height - ph).abs() < 0.5;
                if ui.radio(on, format!("{} ({w:.0} × {h:.0} mm)", tl!(*label))).clicked() {
                    let _ = app.run(
                        "layout.page",
                        json!({"layout": li, "page": id, "orientation": if landscape { "landscape" } else { "portrait" }}),
                    );
                    ui.close();
                }
            }
            ui.separator();
            for o in ["Portrait", "Landscape"] {
                if ui.button(tl!(o)).clicked() {
                    let _ = app.run("layout.page", json!({"layout": li, "orientation": o.to_lowercase()}));
                    ui.close();
                }
            }
        });
        col(ui, |ui| {
            let snap = !app.layout.no_snap;
            let desc = "Snap elements to guides, the page and each other as you move or resize them. Hold Alt to place one freely.";
            small(ui, app, "keep_tool", "Snap", desc, "ui.layoutSnap", Value::Null, snap);
            let shown = !app.layout.no_rulers;
            let desc = "Show rulers. Drag from a ruler to add a guide; drag a guide back onto it to remove it.";
            small(ui, app, "measure", "Rulers", desc, "ui.layoutRulers", Value::Null, shown);
            if !layout.guides.is_empty() {
                small(
                    ui,
                    app,
                    "delete",
                    "Clear Guides",
                    "Remove every guide from this layout.",
                    "layout.guides",
                    json!({"layout": li, "clear": true}),
                    false,
                );
            }
        });
    });
    group(ui, "Map Frames", |ui| {
        let draw_frame = app.layout.tool.is_some();
        let (primary, _) = big_split_menu(
            ui,
            app,
            "map",
            "Map\nFrame",
            "Add a rectangular map frame showing a map of the project.",
            "layout.addFrame",
            json!({"layout": li, "shape": "rectangle"}),
            draw_frame,
            "Map Frame options",
            "Choose a map frame shape.",
            true,
            |ui, app| {
                for (shape, label) in [("rectangle", "Rectangle"), ("circle", "Circle"), ("ellipse", "Ellipse")] {
                    if ui.button(tl!(label)).clicked() {
                        app.layout.cancel_tool();
                        if let Ok(v) = app.run("layout.addFrame", json!({"layout": li, "shape": shape})) {
                            app.layout.selected = v["element"].as_u64().map(|v| v as u32);
                        }
                        ui.close();
                    }
                }
                for (shape, label) in [("polygon", "Polygon (draw)"), ("lasso", "Lasso (draw)")] {
                    if ui.radio(app.layout.tool == Some(shape), tl!(label)).clicked() {
                        app.layout.cancel_tool();
                        let _ = app.run("ui.layoutTool", json!({"shape": shape}));
                        ui.close();
                    }
                }
            },
        );
        if primary.clicked() {
            app.layout.cancel_tool();
            app.layout.selected = app.session.project.layouts.get(li).and_then(|l| l.elements.last()).map(|e| e.id);
        }
        let navigate = app.layout.navigate;
        let desc = "Drag inside map frames to pan their maps and turn the wheel to zoom them, instead of moving them. Esc stops.";
        big(ui, app, "explore", "Pan/Zoom\nMap", desc, "ui.layoutNavigate", Value::Null, navigate, false);
        col(ui, |ui| {
            let has_grid =
                frame.and_then(|f| layout.element(f)).is_some_and(|e| matches!(&e.kind, ElementKind::MapFrame { grid: Some(_), .. }));
            if small(
                ui,
                app,
                "graticule",
                "Grid",
                "Draw a labelled latitude/longitude grid over the map frame.",
                "layout.addElement",
                json!({"layout": li, "frame": frame, "kind": "grid", "off": has_grid}),
                has_grid,
            )
            .clicked()
            {
                app.layout.selected = frame;
            }
            let others: Vec<(u32, String)> = layout
                .elements
                .iter()
                .filter(|e| Some(e.id) != frame && matches!(e.kind, ElementKind::MapFrame { .. }))
                .map(|e| (e.id, e.name.clone()))
                .collect();
            small_menu(ui, app, "g_rect", "Extent Indicator", "Outline another frame's extent in this map frame.", |ui, app| {
                if others.is_empty() {
                    ui.label(egui::RichText::new(tl!("Add a second map frame first")).weak());
                }
                for (id, name) in &others {
                    if ui.button(format!("{name} ({id})")).clicked() {
                        add(app, json!({"kind": "extentIndicator", "of": id}));
                        ui.close();
                    }
                }
            });
            let names: Vec<String> = frame
                .and_then(|f| match layout.element(f).map(|e| &e.kind) {
                    Some(ElementKind::MapFrame { map, .. }) => app.session.project.maps.iter().find(|m| m.name.eq_ignore_ascii_case(map)),
                    _ => None,
                })
                .map(|m| m.layers.iter().filter(|l| !l.is_raster() && !l.graphics).map(|l| l.name.clone()).collect())
                .unwrap_or_default();
            small_menu(ui, app, "bookmark", "Map Series", "One page per feature of an index layer.", |ui, app| {
                for n in &names {
                    if ui.radio(layout.series.as_ref().is_some_and(|s| s.layer == *n), n.as_str()).clicked() {
                        let _ = app.run("layout.series", json!({"layout": li, "layer": n, "frame": frame}));
                        ui.close();
                    }
                }
                if layout.series.is_some() && ui.button(tl!("Turn Off")).clicked() {
                    let _ = app.run("layout.series", json!({"layout": li, "off": true}));
                    ui.close();
                }
            });
        });
        if layout.series.is_some() {
            col(ui, |ui| {
                small(
                    ui,
                    app,
                    "prev_extent",
                    "Previous Page",
                    "Show the previous map series page.",
                    "layout.seriesPage",
                    json!({"layout": li, "page": "previous"}),
                    false,
                );
                small(
                    ui,
                    app,
                    "next_extent",
                    "Next Page",
                    "Show the next map series page.",
                    "layout.seriesPage",
                    json!({"layout": li, "page": "next"}),
                    false,
                );
                small(
                    ui,
                    app,
                    "redo",
                    "Refresh",
                    "Refresh the map series from its index layer.",
                    "layout.seriesRefresh",
                    json!({"layout": li}),
                    false,
                );
            });
        }
    });
    group(ui, "Surrounds", |ui| {
        for (icon, label, kind, desc) in [
            ("north_arrow", "North\nArrow", "northArrow", "Add a north arrow for the map frame."),
            ("scale_bar", "Scale\nBar", "scaleBar", "Add a scale bar for the map frame."),
            ("legend", "Legend", "legend", "Add a legend of the map frame's layers."),
        ] {
            if big(ui, app, icon, label, desc, "layout.addElement", json!({"layout": li, "frame": frame, "kind": kind}), false, false)
                .clicked()
            {
                app.layout.selected = app.session.project.layouts.get(li).and_then(|l| l.elements.last()).map(|e| e.id);
            }
        }
        col(ui, |ui| {
            if small(
                ui,
                app,
                "colorbar",
                "Colorbar",
                "Add a colorbar of a raster layer's colour scale in the map frame.",
                "layout.addElement",
                json!({"layout": li, "frame": frame, "kind": "colorbar"}),
                false,
            )
            .clicked()
            {
                app.layout.selected = app.session.project.layouts.get(li).and_then(|l| l.elements.last()).map(|e| e.id);
            }
            if small(
                ui,
                app,
                "table",
                "Table Frame",
                "Add the attribute table of a layer in the map frame.",
                "layout.addElement",
                json!({"layout": li, "frame": frame, "kind": "table"}),
                false,
            )
            .clicked()
            {
                app.layout.selected = app.session.project.layouts.get(li).and_then(|l| l.elements.last()).map(|e| e.id);
            }
            if small(
                ui,
                app,
                "chart",
                "Chart Frame",
                "Add a bar chart of a layer's numeric field.",
                "layout.addElement",
                json!({"layout": li, "frame": frame, "kind": "chart"}),
                false,
            )
            .clicked()
            {
                app.layout.selected = app.session.project.layouts.get(li).and_then(|l| l.elements.last()).map(|e| e.id);
            }
        });
        col(ui, |ui| {
            small_menu(ui, app, "add_data", "More", "Additional surrounds.", |ui, app| {
                if ui.button(tl!("Neatline")).clicked() {
                    add(app, json!({"kind": "rectangle"}));
                    ui.close();
                }
                if ui.button(tl!("Backdrop")).clicked() {
                    add(app, json!({"kind": "rectangle", "fill": "#F1F4F8", "width": 0}));
                    ui.close();
                }
            });
        });
    });
    group(ui, "Graphics", |ui| {
        let (primary, _) = big_split_menu(
            ui,
            app,
            "g_text",
            "Text",
            "Add a text element.",
            "layout.addElement",
            json!({"layout": li, "frame": frame, "kind": "text"}),
            false,
            "Text options",
            "Add text that updates: page, date, scale, map and more.",
            true,
            |ui, app| {
                for (token, what) in [
                    ("{date}", "Current date"),
                    ("{time}", "Current time"),
                    ("{scale}", "Map scale"),
                    ("{map}", "Map name"),
                    ("{crs}", "Coordinate system"),
                    ("{page} / {pages}", "Page number"),
                    ("{pageName}", "Map series page name"),
                    ("{layout}", "Layout name"),
                ] {
                    if ui.button(format!("{}  {token}", tl!(what))).clicked() {
                        add(app, json!({"kind": "dynamicText", "text": token}));
                        ui.close();
                    }
                }
            },
        );
        if primary.clicked() {
            app.layout.selected = app.session.project.layouts.get(li).and_then(|l| l.elements.last()).map(|e| e.id);
        }
        big(ui, app, "picture", "Picture", "Add a PNG or JPEG picture to the page.", "ui.layoutPicture", Value::Null, false, false);
        // The default rectangle sits in the middle of the page, ready to move and size.
        let (pw, ph) = (layout.width, layout.height);
        let (w, h) = (pw * 0.25, ph * 0.2);
        let (primary, _) = big_split_menu(
            ui,
            app,
            "g_rect",
            "Shapes",
            "Add a rectangle, to frame or highlight part of the page.",
            "layout.addElement",
            json!({"layout": li, "kind": "rectangle", "x": (pw - w) / 2.0, "y": (ph - h) / 2.0, "w": w, "h": h}),
            false,
            "Shapes options",
            "Add a rectangle, circle, ellipse, line or arrow.",
            true,
            |ui, app| {
                if ui.button(tl!("Rectangle")).clicked() {
                    add(app, json!({"kind": "rectangle", "x": (pw - w) / 2.0, "y": (ph - h) / 2.0, "w": w, "h": h}));
                    ui.close();
                }
                for (kind, label) in [("circle", "Circle"), ("ellipse", "Ellipse")] {
                    if ui.button(tl!(label)).clicked() {
                        add(app, json!({"kind": kind}));
                        ui.close();
                    }
                }
                ui.menu_button(tl!("Line"), |ui| {
                    for d in rustgis_engine::map::layout::LineDir::ALL {
                        if ui.button(tl!(d.label())).clicked() {
                            add(app, json!({"kind": "line", "dir": d.code()}));
                            ui.close();
                        }
                    }
                });
                if ui.button(tl!("Arrow")).clicked() {
                    add(app, json!({"kind": "arrow", "angle": 0}));
                    ui.close();
                }
            },
        );
        if primary.clicked() {
            app.layout.selected = app.session.project.layouts.get(li).and_then(|l| l.elements.last()).map(|e| e.id);
        }
    });
    group(ui, "Output", |ui| {
        big(ui, app, "export", "Export", "Export the layout as PDF or PNG.", "ui.exportLayout", Value::Null, false, false);
        big(ui, app, "print", "Print", "Print the layout.", "ui.printLayout", Value::Null, false, false);
        big(ui, app, "map", "Close\nLayout", "Close the layout and show the map.", "layout.close", Value::Null, false, false);
    });
}

fn plugins_tab(app: &mut RustGisApp, ui: &mut Ui) {
    group(ui, "Plugins", |ui| {
        big(
            ui,
            app,
            "plugin",
            "Plugin\nManager",
            "Browse, install, update and remove plugins.",
            "ui.plugins",
            json!({"open": true}),
            false,
            false,
        );
        col(ui, |ui| {
            small(
                ui,
                app,
                "plugin_add",
                "Install From File…",
                "Install a plugin from a .rgplugin file.",
                "ui.installPlugin",
                Value::Null,
                false,
            );
        });
    });
    // A group per enabled plugin with commands, in the interface language when it has one
    // (plugins with a tab of their own are on that tab instead).
    let lang = app.lang().code();
    // Its tools too: each opens in the Tools pane.
    for p in rustgis_engine::plugins::list()
        .into_iter()
        .filter(|p| p.enabled && p.manifest.tab.is_none() && !(p.manifest.commands.is_empty() && p.manifest.tools.is_empty()))
    {
        let m = &p.manifest;
        group(ui, m.translate(lang, &m.name), |ui| {
            // Its panel first, when it has one.
            if let Some(d) = &m.panel {
                let (icon, label, help) = panel_button(m, d, lang);
                let open = panel_open(app, &m.id);
                big(ui, app, icon, &label, &help, "ui.pane.plugin", json!({"id": m.id}), open, false);
            }
            for c in &m.commands {
                let icon = if crate::icons::ALL.contains(&c.icon.as_str()) { c.icon.as_str() } else { "plugin" };
                let id = rustgis_engine::plugins::command_id(&m.id, &c.id);
                big(ui, app, icon, m.translate(lang, &c.label), m.translate(lang, &c.help), &id, Value::Null, false, false);
            }
            for t in &m.tools {
                let id = rustgis_engine::plugins::command_id(&m.id, &t.id);
                let label = m.translate(lang, &t.name).replacen(' ', "\n", 1);
                big(ui, app, "toolbox", &label, m.translate(lang, &t.summary), "ui.gp.open", json!({"tool": id}), false, false);
            }
        });
    }
}

/// The button that opens a plugin's panel: its icon, label (the panel's title, on two lines when
/// long) and help, in language `lang`.
fn panel_button(
    m: &rustgis_engine::plugins::Manifest,
    d: &rustgis_engine::plugins::PanelDecl,
    lang: &str,
) -> (&'static str, String, String) {
    let icon = crate::icons::ALL.iter().copied().find(|i| *i == d.icon).unwrap_or("plugin");
    let title = m.translate(lang, &d.title);
    let label = if title.chars().count() > 12 { title.replacen(' ', "\n", 1) } else { title.to_string() };
    let help = if d.help.is_empty() { tl!("Open the plugin's panel.").to_string() } else { m.translate(lang, &d.help).to_string() };
    (icon, label, help)
}

/// Whether plugin `id`'s panel is in view.
fn panel_open(app: &RustGisApp, id: &str) -> bool {
    app.ui.right == Some(crate::RightPane::Plugin) && app.ui.plugin_panel.plugin.as_deref() == Some(id)
}

/// A plugin's own tab: its groups of commands, tools and menus.
fn plugin_tab(app: &mut RustGisApp, ui: &mut Ui, plugin: &str) {
    use rustgis_engine::plugins::{Entry, ItemDecl, command_id};
    let Some(p) = rustgis_engine::plugins::get(plugin).filter(|p| p.enabled) else { return };
    let Some(tab) = p.manifest.tab.as_ref() else { return };
    let m = &p.manifest;
    let lang = app.lang().code();
    // What a button runs: a command, or the Tools pane open on a tool.
    let target = |e: Entry<'_>| -> (String, String, String, String, Value) {
        match e {
            Entry::Command(c) => {
                let icon = if crate::icons::ALL.contains(&c.icon.as_str()) { c.icon.clone() } else { "plugin".into() };
                (
                    icon,
                    m.translate(lang, &c.label).to_string(),
                    m.translate(lang, &c.help).to_string(),
                    command_id(&m.id, &c.id),
                    Value::Null,
                )
            }
            Entry::Tool(t) => (
                "toolbox".into(),
                m.translate(lang, &t.name).replacen(' ', "\n", 1),
                m.translate(lang, &t.summary).to_string(),
                "ui.gp.open".into(),
                json!({"tool": command_id(&m.id, &t.id)}),
            ),
            Entry::Panel(d) => {
                let (icon, label, help) = panel_button(m, d, lang);
                (icon.into(), label, help, "ui.pane.plugin".into(), json!({"id": m.id}))
            }
        }
    };
    let open = panel_open(app, &m.id);
    for g in &tab.groups {
        group(ui, m.translate(lang, &g.label), |ui| {
            for item in &g.items {
                match item {
                    ItemDecl::Entry(e) => {
                        if let Some((icon, label, help, id, params)) = m.entry(e).map(target) {
                            let checked = open && id == "ui.pane.plugin";
                            big(ui, app, &icon, &label, &help, &id, params, checked, false);
                        }
                    }
                    ItemDecl::Menu(menu) => {
                        let icon = if crate::icons::ALL.contains(&menu.icon.as_str()) { menu.icon.as_str() } else { "plugin" };
                        big_menu(ui, app, icon, m.translate(lang, &menu.menu), m.translate(lang, &menu.help), |ui, app| {
                            for e in &menu.items {
                                if let Some((icon, label, _, id, params)) = m.entry(e).map(target) {
                                    menu_item(ui, app, &icon, &label.replace('\n', " "), &id, params);
                                }
                            }
                        });
                    }
                }
            }
        });
    }
}

fn settings_tab(app: &mut RustGisApp, ui: &mut Ui) {
    group(ui, "Customize", |ui| {
        let hidden = app.ui.hidden_tabs.clone();
        // RustGIS's tabs and plugins' tabs.
        let mut all: Vec<(String, String)> =
            TABS.iter().filter(|t| **t != "Layout" && !FIXED_TABS.contains(*t)).map(|t| (t.to_string(), tl!(*t).to_string())).collect();
        all.extend(plugin_tabs(app));
        big_menu(ui, app, "settings", "Ribbon", "Choose which tabs the ribbon shows.", move |ui, app| {
            for (tab, caption) in &all {
                let mut on = !hidden.iter().any(|h| h == tab);
                if ui.checkbox(&mut on, caption.as_str()).clicked() {
                    let _ = app.run("ui.customizeRibbon", json!({"tab": tab}));
                }
            }
        });
        let qa = app.ui.quick_access.clone();
        big_menu(ui, app, "command", "Quick\nAccess", "Choose extra buttons for the Quick Access Toolbar.", move |ui, app| {
            for (id, label) in QUICK {
                let mut on = qa.iter().any(|c| c == id);
                if ui.checkbox(&mut on, tl!(*label)).clicked() {
                    let _ = app.run("ui.quickAccess", json!({"command": id}));
                }
            }
        });
    });
    group(ui, "Display", |ui| {
        col(ui, |ui| {
            small(ui, app, "dark", "Dark Mode", "Switch between the light and dark theme.", "ui.theme", Value::Null, app.ui.dark);
            small(ui, app, "globe_view", "GPU Rendering", "Draw the map with the GPU.", "ui.map.gpu", Value::Null, app.map.gpu_enabled);
            small(
                ui,
                app,
                "identify",
                "Item Pop-ups",
                "Show details when hovering items in the Browser view.",
                "ui.itemPopups",
                Value::Null,
                app.ws.item_popups,
            );
        });
        col(ui, |ui| {
            small_menu(ui, app, "measure", "Measurement Units", "Choose the units for measurements and the map scale bar.", |ui, app| {
                for (units, label, imperial) in [("metric", "Metric", false), ("imperial", "Imperial", true)] {
                    if ui.radio(app.ui.measurement_imperial == imperial, tl!(label)).clicked() {
                        let _ = app.run("ui.measurementUnits", json!({"units": units}));
                        ui.close();
                    }
                }
            });
            small_menu(ui, app, "add_data", "Feature Cache", "Prepare geometry for the current projection.", |ui, app| {
                menu_item(ui, app, "add_data", "Fill", "cache.fill", Value::Null);
                menu_item(ui, app, "clear_sel", "Empty", "cache.empty", Value::Null);
                menu_item(ui, app, "close", "Cancel", "cache.cancel", Value::Null);
                let mut auto = app.session.auto_cache;
                if ui.checkbox(&mut auto, tl!("Auto Cache")).clicked() {
                    let _ = app.run("cache.auto", Value::Null);
                }
            });
        });
    });
    group(ui, "Language", |ui| {
        language_menu(app, ui);
        worldview_menu(app, ui);
    });
    group(ui, "Start-up", |ui| start_project_menu(app, ui));
}

/// Settings › Start-up › Start With: the World Atlas sample, an empty project, or (in the
/// desktop app) a project picked once or the last one opened or saved (#377).
fn start_project_menu(app: &mut RustGisApp, ui: &mut Ui) {
    use crate::StartWith;
    let with = app.ui.start_with.clone();
    // The button's second line is the file's name, kept short so the ribbon keeps its width.
    let short_name = |p: &str| {
        let name = std::path::Path::new(p).file_name().map_or_else(|| p.to_string(), |n| n.to_string_lossy().into_owned());
        if name.chars().count() > 24 { format!("{}…", name.chars().take(23).collect::<String>()) } else { name }
    };
    let current = match &with {
        StartWith::Sample => tl!("World Atlas").to_string(),
        StartWith::Blank => tl!("Blank Map").to_string(),
        StartWith::Last => tl!("Last Project").to_string(),
        StartWith::File(p) => short_name(p),
    };
    let label = format!("{}\n{current}", tl!("Start With"));
    let files = app.opens_files();
    let tip = if files {
        "What RustGIS opens with: the World Atlas sample, a blank map, a project you choose, or the last project opened or saved."
    } else {
        "What RustGIS opens with: the World Atlas sample or a blank map."
    };
    big_menu(ui, app, "map", &label, tip, move |ui, app| {
        for (w, name) in [(StartWith::Sample, tl!("World Atlas")), (StartWith::Blank, tl!("Blank Map"))] {
            if ui.add(egui::Button::selectable(with == w, name)).clicked() {
                let _ = app.run("ui.startup", json!({"project": w.json()}));
                ui.close();
            }
        }
        if !files {
            return;
        }
        // Last Project: offered once there is one (or while it's the choice).
        let last = app.ui.last_project.clone();
        let item = ui
            .add_enabled(last.is_some() || with == StartWith::Last, egui::Button::selectable(with == StartWith::Last, tl!("Last Project")))
            .on_hover_text(last.clone().unwrap_or_else(|| tl!("No project opened or saved yet").to_string()))
            .on_disabled_hover_text(tl!("No project opened or saved yet"));
        if item.clicked() {
            let _ = app.run("ui.startup", json!({"project": "last"}));
            ui.close();
        }
        let chosen = match &with {
            StartWith::File(p) => p.clone(),
            _ => String::new(),
        };
        let item = ui.add(egui::Button::selectable(matches!(with, StartWith::File(_)), tl!("Choose Project…")));
        let item = if chosen.is_empty() { item } else { item.on_hover_text(chosen) };
        if item.clicked() {
            ui.close();
            if let Some(path) = crate::dialogs::pick_open(&["rgis", "rgisx"], tl!("RustGIS project"))
                && let Err(e) = app.run("ui.startup", json!({"project": {"path": path}}))
            {
                app.session.notify(e, true);
            }
        }
    });
    start_projection_menu(app, ui);
    start_extent_menu(app, ui);
}

/// Settings › Start-up › Extent: the full extent or a saved view.
fn start_extent_menu(app: &mut RustGisApp, ui: &mut Ui) {
    let saved = app.ui.start_extent;
    let current = if saved.is_some() { tl!("Saved View") } else { tl!("Full Extent") };
    let label = format!("{}\n{current}", tl!("Extent"));
    big_menu(ui, app, "zoom_full", &label, "Where the map opens: the full extent or a view you saved.", move |ui, app| {
        if ui.add(egui::Button::selectable(saved.is_none(), tl!("Full Extent"))).clicked() {
            let _ = app.run("ui.startup", json!({"extent": null}));
            ui.close();
        }
        if ui.button(tl!("Use Current View")).clicked() {
            if let Err(e) = app.run("ui.startup", json!({"extent": "view"})) {
                app.session.notify(e, true);
            }
            ui.close();
        }
        if let Some(e) = saved {
            ui.separator();
            ui.label(format!("{:.4}, {:.4}, {:.4}, {:.4}", e[0], e[1], e[2], e[3]));
        }
    });
}

/// Whether projection code `current` is menu item `id`: the same code, or that preset with
/// parameters (`robinson +lon_0=150` is Robinson).
fn is_projection(current: &str, id: &str) -> bool {
    current == id || rustgis_engine::map::projection::parse_preset(current).ok().flatten().is_some_and(|(p, _)| p == id)
}

/// Settings › Start-up › Projection: the projection the map opens in.
fn start_projection_menu(app: &mut RustGisApp, ui: &mut Ui) {
    let current = if app.ui.start_projection.is_empty() { "mercator".to_string() } else { app.ui.start_projection.clone() };
    let shown = rustgis_engine::map::Projection::from_code(&current).map(|p| p.label()).unwrap_or_else(|_| current.clone());
    let label = format!("{}\n{}", tl!("Projection"), tl!(shown.as_str()));
    big_form_menu(ui, app, "basemap", &label, "The projection the map opens in.", move |ui, app| {
        let mut items: Vec<(String, String)> = vec![("mercator".into(), "Web Mercator".into()), ("globe".into(), "Globe".into())];
        items.extend(rustgis_engine::map::projection::PRESETS.iter().map(|(id, label, _)| (id.to_string(), label.to_string())));
        for (id, label) in items {
            if ui.add(egui::Button::selectable(is_projection(&current, &id), tl!(label.as_str()))).clicked() {
                let _ = app.run("ui.startup", json!({"projection": id}));
                ui.close();
            }
        }
        ui.separator();
        ui.label(tl!("EPSG or ESRI code"));
        ui.horizontal(|ui| {
            let edit = ui.add(egui::TextEdit::singleline(&mut app.ui.start_projection_input).hint_text("3010").desired_width(90.0));
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button(tl!("Use")).clicked() || enter {
                let code = crate::dialogs::typed_crs(&app.ui.start_projection_input);
                match app.run("ui.startup", json!({"projection": code})) {
                    Ok(_) => {
                        app.ui.start_projection_input.clear();
                        ui.close();
                    }
                    Err(e) => app.session.notify(e, true),
                }
            }
        });
        if let Ok(rustgis_engine::map::Projection::Custom(c)) = rustgis_engine::map::Projection::from_code(&current)
            && (c.starts_with("EPSG:") || c.starts_with("ESRI:"))
        {
            ui.add(egui::Button::selectable(true, c.as_str()));
        }
    });
}

fn help_tab(app: &mut RustGisApp, ui: &mut Ui) {
    group(ui, "Help", |ui| {
        big(ui, app, "help", "Learning\nResources", "Open the RustGIS user guide.", "ui.learn", Value::Null, false, false);
        big(ui, app, "command", "Command\nSearch", "Find any command by name (Ctrl+K).", "ui.commandSearch", Value::Null, false, false);
        col(ui, |ui| {
            small(ui, app, "settings", "Shortcuts", "Keyboard shortcuts.", "ui.shortcuts", Value::Null, false);
            small(
                ui,
                app,
                "dra",
                "Diagnostic Monitor",
                "Frame rate, draw time, data size and memory.",
                "ui.pane.diagnostics",
                Value::Null,
                app.ui.right_open.contains(&RightPane::Diagnostics),
            );
            if !cfg!(target_arch = "wasm32") {
                small(
                    ui,
                    app,
                    "update",
                    "Check for Updates",
                    "See whether a newer RustGIS has been released.",
                    "ui.checkUpdate",
                    Value::Null,
                    false,
                );
            }
        });
    });
}

// ---------------------------------------------------------------------------------------------
// Style and Data groups for the selected layer

pub(crate) const SCALES: [f64; 10] =
    [1_000.0, 10_000.0, 50_000.0, 100_000.0, 500_000.0, 1_000_000.0, 5_000_000.0, 10_000_000.0, 50_000_000.0, 100_000_000.0];

pub(crate) fn scale_label(s: Option<f64>) -> String {
    match s {
        None => tl!("<None>").into(),
        Some(v) => format!("1:{}", fmt_thousands(v)),
    }
}

pub fn fmt_thousands(v: f64) -> String {
    let s = format!("{:.0}", v.abs());
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if v < 0.0 { format!("-{out}") } else { out }
}

fn feature_style_groups(app: &mut RustGisApp, ui: &mut Ui) {
    let Some(layer) = app.session.active_layer.and_then(|id| app.session.map().layer(id)).cloned() else { return };
    visibility_and_effects(app, ui, &layer);
    drawing_group(app, ui, &layer);
    effects_groups(app, ui, &layer);
    layer_group(app, ui, &layer);
}

/// The polygon layers that could mask `layer`.
fn mask_menu(ui: &mut Ui, app: &mut RustGisApp, layer: &rustgis_engine::map::Layer) {
    let polys: Vec<String> = app
        .session
        .map()
        .layers
        .iter()
        .filter(|l| l.id != layer.id && l.geometry_type() == GeometryType::Polygon)
        .map(|l| l.name.clone())
        .collect();
    let cur = layer.mask.clone();
    small_menu(ui, app, "polygon_layer", "Masking", "Show the layer only inside another layer's polygons.", move |ui, app| {
        if ui.radio(cur.is_none(), tl!("No mask")).clicked() {
            let _ = app.run("layer.mask", json!({"mask": null}));
            ui.close();
        }
        for n in &polys {
            if ui.radio(cur.as_deref() == Some(n.as_str()), n.as_str()).clicked() {
                let _ = app.run("layer.mask", json!({"mask": n}));
                ui.close();
            }
        }
    });
}

/// Masking, display filters, aggregation and extrusion for feature layers.
fn effects_groups(app: &mut RustGisApp, ui: &mut Ui, layer: &rustgis_engine::map::Layer) {
    let gt = layer.geometry_type();
    group(ui, "Effects", |ui| {
        col(ui, |ui| {
            mask_menu(ui, app, layer);
            let filters = layer.display_filters.len();
            let label = if filters > 0 {
                crate::i18n::fmt("Display Filters ({n})", &[("n", &filters.to_string())])
            } else {
                tl!("Display Filters").to_string()
            };
            small_form_menu(ui, app, "filter", &label, "Show different features at different scales.", |ui, app| {
                let input_label =
                    ui.label(egui::RichText::new(tl!("Below 1:5,000,000 show everything; beyond it only what matches:")).small().weak());
                ui.add(egui::TextEdit::singleline(&mut app.forms.expression).hint_text("pop_max > 1000000").desired_width(220.0))
                    .labelled_by(input_label.id);
                if ui.button(tl!("Set Filter")).clicked() {
                    let w = app.forms.expression.trim().to_string();
                    let _ =
                        app.run("layer.displayFilters", json!({"filters": [{"name": "Small scales", "maxScale": 5_000_000, "where": w}]}));
                    ui.close();
                }
                if ui.button(tl!("Clear Filters")).clicked() {
                    let _ = app.run("layer.displayFilters", json!({"filters": []}));
                    ui.close();
                }
            });
            if gt == GeometryType::Point {
                let cur = layer.aggregation;
                small_menu(ui, app, "count", "Aggregation", "Draw points as bins coloured by how many fall in each.", move |ui, app| {
                    for (label, shape) in [("None", "none"), ("Hexagons", "hexagon"), ("Squares", "square")] {
                        let on = match shape {
                            "none" => cur.is_none(),
                            "hexagon" => cur.is_some_and(|a| a.hexagons),
                            _ => cur.is_some_and(|a| !a.hexagons),
                        };
                        if ui.radio(on, tl!(label)).clicked() {
                            let _ = app.run("layer.aggregation", json!({"shape": shape, "size": cur.map_or(24.0, |a| a.size)}));
                            ui.close();
                        }
                    }
                });
            }
        });
    });
    if gt == GeometryType::Polygon {
        let x = layer.extrusion.clone();
        let numeric: Vec<String> = layer.dataset.fields.iter().filter(|f| f.ty.is_numeric()).map(|f| f.name.clone()).collect();
        group(ui, "3D", |ui| {
            let cur = x.clone();
            big_menu(ui, app, "polygon_layer", "Extrusion", "Raise polygons by a height or a field.", move |ui, app| {
                if ui.radio(cur.is_none(), tl!("None")).clicked() {
                    let _ = app.run("layer.extrusion", json!({"type": "none"}));
                    ui.close();
                }
                if ui.radio(cur.as_ref().is_some_and(|x| x.field.is_none()), tl!("Constant height")).clicked() {
                    let _ = app.run("layer.extrusion", json!({"type": "height", "height": 50_000}));
                    ui.close();
                }
                for f in &numeric {
                    if ui
                        .radio(
                            cur.as_ref().and_then(|x| x.field.as_deref()) == Some(f.as_str()),
                            crate::i18n::fmt("By {field}", &[("field", f)]),
                        )
                        .clicked()
                    {
                        let _ = app.run("layer.extrusion", json!({"type": "field", "field": f}));
                        ui.close();
                    }
                }
            });
            if let Some(xx) = &x {
                ui.vertical(|ui| {
                    ui.add_space(8.0);
                    let mut k = xx.factor;
                    ui.horizontal(|ui| {
                        let input_label = ui.label(egui::RichText::new(tl!("Factor")).size(11.5));
                        // Kept across a drag until it is let go (#607).
                        let drag = Dragged::lend(ui, egui::Id::unique(("extrusion_factor", layer.id)), &mut k);
                        let r = ui
                            .add(egui::DragValue::new(&mut k).range(0.0..=1e6).speed(xx.factor.max(0.0001) * 0.02))
                            .labelled_by(input_label.id);
                        if drag.settled(ui, &r, &mut k) && k != xx.factor {
                            let t = if xx.field.is_some() { "field" } else { "height" };
                            let _ = app.run("layer.extrusion", json!({"type": t, "field": xx.field, "factor": k}));
                        }
                    });
                });
                col(ui, |ui| {
                    small(
                        ui,
                        app,
                        "single",
                        "Face Culling",
                        "Hide walls facing away from you.",
                        "layer.faces",
                        json!({"culling": !xx.face_culling}),
                        xx.face_culling,
                    );
                    small(
                        ui,
                        app,
                        "brightness",
                        "Lighting",
                        "Shade walls by the way they face the light.",
                        "layer.faces",
                        json!({"lighting": !xx.lighting}),
                        xx.lighting,
                    );
                    small(
                        ui,
                        app,
                        "contrast",
                        "Cast Shadow",
                        "Extruded shapes cast shadows on the map.",
                        "layer.lightShadow",
                        json!({"shadows": !xx.shadows}),
                        xx.shadows,
                    );
                });
            }
        });
    }
}

fn imagery_groups(app: &mut RustGisApp, ui: &mut Ui) {
    group(ui, "Imagery", |ui| {
        let raster_functions = app.ui.right_open.contains(&RightPane::Workflows)
            && matches!(app.workflow.section, "rasterFunctions" | "functionEditor" | "indices" | "process" | "pixels");
        big_split_menu(
            ui,
            app,
            "relief",
            "Raster\nFunctions",
            "Slope, aspect, hillshade, NDVI, band math, thresholds.",
            "ui.pane.workflows",
            json!({"section": "rasterFunctions"}),
            raster_functions,
            "Raster Functions options",
            "Raster function editing, indices, processing and pixel editing.",
            true,
            |ui, app| {
                for (icon, label, section) in [
                    ("bands", "Function Editor", "functionEditor"),
                    ("stretch", "Indices", "indices"),
                    ("resample", "Process", "process"),
                    ("edit", "Pixel Editor", "pixels"),
                ] {
                    menu_item(ui, app, icon, label, "ui.pane.workflows", json!({"section": section}));
                }
            },
        );
        big_menu(ui, app, "classify", "Classify", "Classify imagery with unsupervised, supervised or deep learning tools.", |ui, app| {
            for (icon, label, section) in [
                ("classify", "Classification Wizard", "classify"),
                ("unique", "Classification Tools", "supervised"),
                ("modelbuilder", "Deep Learning Tools", "deepLearning"),
            ] {
                menu_item(ui, app, icon, label, "ui.pane.workflows", json!({"section": section}));
            }
        });
        big_menu(ui, app, "raster_layer", "Imagery\nWorkflows", "Georeference, compare, measure and publish imagery.", |ui, app| {
            for (icon, label, section) in [
                ("goto_xy", "Georeference", "georeference"),
                ("contrast", "Change Detection", "change"),
                ("graduated", "Spectral Analysis", "spectral"),
                ("history", "Trend Analysis", "trend"),
                ("raster_layer", "Ortho Mapping", "ortho"),
                ("relief", "Reality Mapping", "reality"),
                ("measure", "Mensuration", "mensuration"),
                ("raster_layer", "Create Hosted Imagery", "hosted"),
            ] {
                menu_item(ui, app, icon, label, "ui.pane.workflows", json!({"section": section}));
            }
        });
    });
}

/// The Visibility group (scale range and transparency) shared by feature and raster layers.
fn visibility_and_effects(app: &mut RustGisApp, ui: &mut Ui, layer: &rustgis_engine::map::Layer) {
    group(ui, "Visibility", |ui| {
        ui.vertical(|ui| {
            ui.add_space(6.0);
            for (label, cur, key) in [("Out Beyond", layer.min_scale, "min"), ("In Beyond", layer.max_scale, "max")] {
                ui.horizontal(|ui| {
                    let input_label = ui.label(egui::RichText::new(tl!(label)).size(11.5));
                    egui::ComboBox::from_id_salt(format!("scale_{key}"))
                        .selected_text(scale_label(cur))
                        .width(118.0)
                        .show_ui(ui, |ui| {
                            let mut choices: Vec<Option<f64>> = vec![None];
                            choices.extend(SCALES.iter().map(|s| Some(*s)));
                            for c in choices {
                                if ui.selectable_label(c == cur, scale_label(c)).clicked() {
                                    let (mut min, mut max) = (layer.min_scale, layer.max_scale);
                                    if key == "min" {
                                        min = c;
                                    } else {
                                        max = c;
                                    }
                                    let _ = app.run("layer.scaleRange", json!({"min": min, "max": max}));
                                }
                            }
                        })
                        .response
                        .labelled_by(input_label.id);
                });
            }
        });
        big(ui, app, "scale_range", "Clear\nLimits", "Draw the layer at every scale.", "layer.scaleRange", json!({}), false, false);
        ui.vertical(|ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let t = Tokens::get(ui.ctx());
                icons::paint(ui.painter(), Rect::from_min_size(ui.cursor().min, vec2(20.0, 20.0)), "transparency", t.icon, t.accent);
                ui.add_space(24.0);
                ui.label(egui::RichText::new(tl!("Transparency")).size(11.5));
            });
            let mut v = layer.transparency;
            ui.spacing_mut().slider_width = 130.0;
            let r = ui.add(egui::Slider::new(&mut v, 0.0..=100.0).suffix(" %").integer()).on_hover_text(tl!("Transparency"));
            live_drag(
                app,
                ui,
                &r,
                layer.transparency as f64,
                v as f64,
                |app, x| {
                    if let Some(l) = app.session.active_layer.and_then(|id| app.session.map_mut().layer_mut(id)) {
                        l.transparency = x as f32;
                    }
                },
                |app, x| {
                    let _ = app.run("layer.transparency", json!({"value": x}));
                },
            );
        });
    });
}

/// A dragged control with a live preview: `set` writes the value straight into the project while
/// dragging, and on release the pre-drag value is put back before `commit` runs the command, so
/// the command's undo snapshot holds the old value.
pub(crate) fn live_drag(
    app: &mut RustGisApp,
    ui: &Ui,
    resp: &egui::Response,
    before: f64,
    value: f64,
    set: impl Fn(&mut RustGisApp, f64),
    commit: impl FnOnce(&mut RustGisApp, f64),
) {
    let key = resp.id.with("live_drag_original");
    if resp.drag_started() {
        ui.data_mut(|d| d.insert_temp(key, before));
    }
    if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
        if let Some(original) = ui.data_mut(|d| d.remove_temp::<f64>(key)) {
            set(app, original);
        }
        commit(app, value);
    } else if resp.dragged() {
        set(app, value);
        app.session.revision = app.session.revision.wrapping_add(1);
    }
}

/// Style ribbon controls for the layer's renderer, fill, outline and marker size.
fn drawing_group(app: &mut RustGisApp, ui: &mut Ui, layer: &rustgis_engine::map::Layer) {
    group(ui, "Drawing", |ui| {
        big(
            ui,
            app,
            "symbology",
            "Symbology",
            "Open the Symbology pane.",
            "ui.pane.symbology",
            Value::Null,
            app.ui.right == Some(RightPane::Symbology),
            false,
        );
        let numeric: Vec<String> = layer.dataset.fields.iter().filter(|f| f.ty.is_numeric()).map(|f| f.name.clone()).collect();
        let text_f: Vec<String> = layer.dataset.fields.iter().map(|f| f.name.clone()).collect();
        let is_point = layer.geometry_type() == GeometryType::Point;
        big_menu(ui, app, renderer_icon(&layer.renderer), "Primary\nSymbology", "Choose how the layer is drawn.", move |ui, app| {
            if ui.button(tl!("Single Symbol")).clicked() {
                let _ = app.run("layer.symbology", json!({"type": "single"}));
                ui.close();
            }
            ui.menu_button(tl!("Unique Values"), |ui| {
                for f in &text_f {
                    if ui.button(f).clicked() {
                        let _ = app.run("layer.symbology", json!({"type": "unique", "field": f}));
                        ui.close();
                    }
                }
            });
            ui.menu_button(tl!("Graduated Colors"), |ui| {
                for f in &numeric {
                    if ui.button(f).clicked() {
                        let _ = app.run("layer.symbology", json!({"type": "graduated", "field": f}));
                        ui.close();
                    }
                }
            });
            if is_point {
                ui.menu_button(tl!("Proportional Symbols"), |ui| {
                    for f in &numeric {
                        if ui.button(f).clicked() {
                            let _ = app.run("layer.symbology", json!({"type": "proportional", "field": f}));
                            ui.close();
                        }
                    }
                });
            }
        });
        col(ui, |ui| {
            let mut c = crate::theme::c32(layer.renderer.base_symbol().fill);
            ui.horizontal(|ui| {
                if crate::widgets::color_button(ui, &mut c).changed() {
                    let _ = app.run("layer.color", json!({"color": crate::theme::rgba(c).hex8()}));
                }
                ui.label(egui::RichText::new(tl!("Color")).size(11.5));
            });
            let mut w = layer.renderer.base_symbol().stroke_width;
            ui.horizontal(|ui| {
                if ui.add(egui::DragValue::new(&mut w).range(0.0..=10.0).speed(0.05).suffix(" pt")).changed() {
                    let _ = app.run("layer.outline", json!({"width": w}));
                }
                ui.label(egui::RichText::new(tl!("Outline")).size(11.5));
            });
            if layer.geometry_type() == GeometryType::Point {
                let mut s = layer.renderer.base_symbol().size;
                ui.horizontal(|ui| {
                    if ui.add(egui::DragValue::new(&mut s).range(1.0..=60.0).speed(0.1).suffix(" pt")).changed() {
                        let _ = app.run("layer.marker", json!({"size": s}));
                    }
                    ui.label(egui::RichText::new(tl!("Size")).size(11.5));
                });
            } else if layer.geometry_type() == GeometryType::Polygon && !layer.graphics {
                small(
                    ui,
                    app,
                    "no_fill",
                    "No Fill",
                    "Draw only polygon outlines, keeping their colors and widths.",
                    "layer.noFill",
                    Value::Null,
                    false,
                );
            }
        });
    });
}

fn layer_group(app: &mut RustGisApp, ui: &mut Ui, layer: &rustgis_engine::map::Layer) {
    group(ui, "Layer", |ui| {
        big(ui, app, "zoom_full", "Zoom To\nLayer", "Zoom to the layer's extent.", "layer.zoomTo", Value::Null, false, false);
        col(ui, |ui| {
            if layer.is_raster() {
                small(
                    ui,
                    app,
                    "symbology",
                    "Symbology",
                    "Open the Symbology pane.",
                    "ui.pane.symbology",
                    Value::Null,
                    app.ui.right == Some(RightPane::Symbology),
                );
            }
            small(ui, app, "eye", "Visible", "Show or hide the layer.", "layer.visibility", Value::Null, layer.visible);
            small(ui, app, "delete", "Remove", "Remove the layer from the map.", "layer.remove", Value::Null, false);
        });
        col(ui, |ui| {
            let pr = layer.priority;
            small_menu(ui, app, "up", "Draw Priority", "Draw the selected layer above or below others.", move |ui, app| {
                menu_item(ui, app, "up", "Raise Priority", "layer.priority", json!({"value": pr + 1}));
                menu_item(ui, app, "down", "Lower Priority", "layer.priority", json!({"value": pr - 1}));
            });
        });
    });
}

fn raster_style_groups(app: &mut RustGisApp, ui: &mut Ui) {
    let Some(layer) = app.session.active_layer.and_then(|id| app.session.map().layer(id)).cloned() else { return };
    let Some(rl) = layer.raster.clone() else { return };
    visibility_and_effects(app, ui, &layer);
    let Some(data) = rl.data.clone() else {
        group(ui, "Rendering", |ui| {
            ui.label(egui::RichText::new(tl!("The raster's data couldn't be read.")).size(11.5));
        });
        return;
    };
    let style = rl.style.clone();
    group(ui, "Compare", |ui| {
        let on = app.session.map().swipe.is_some_and(|(id, _)| id == layer.id);
        big(
            ui,
            app,
            "contrast",
            "Swipe",
            "Show the layer only left of a divider, to compare it with what's beneath.",
            "layer.swipe",
            if on { json!({"off": true}) } else { json!({"position": 0.5}) },
            on,
            false,
        );
        if on {
            let mut x = app.session.map().swipe.map_or(0.5, |(_, x)| x);
            ui.vertical(|ui| {
                ui.add_space(14.0);
                if ui.add(egui::Slider::new(&mut x, 0.0..=1.0).show_value(false)).on_hover_text(tl!("Swipe")).changed() {
                    let _ = app.run("layer.swipe", json!({"position": x}));
                }
            });
        }
    });
    group(ui, "Raster Effects", |ui| {
        col(ui, |ui| {
            let cur = layer.blend;
            small_menu(ui, app, "transparency", "Layer Blend", "How the layer's colours combine with what's beneath.", move |ui, app| {
                for b in rustgis_engine::map::effects::Blend::ALL {
                    if ui.radio(cur == b, tl!(b.label())).clicked() {
                        let _ = app.run("layer.blend", json!({"mode": b.code()}));
                        ui.close();
                    }
                }
            });
            mask_menu(ui, app, &layer);
            let mut a = style.rotation;
            ui.horizontal(|ui| {
                // Kept across a drag until it is let go (#607).
                let drag = Dragged::lend(ui, egui::Id::unique(("raster_rotation", layer.id)), &mut a);
                let r = ui.add(egui::DragValue::new(&mut a).range(0.0..=359.0).speed(1.0).suffix("°"));
                if drag.settled(ui, &r, &mut a) && a != style.rotation {
                    let _ = app.run("raster.rotation", json!({"angle": a}));
                }
                ui.label(egui::RichText::new(tl!("Rotation")).size(11.5));
            });
        });
    });
    group(ui, "Rendering", |ui| {
        let n = data.band_count();
        let has_categories = data.bands.first().is_some_and(|b| b.stats.categories.is_some());
        let palette = matches!(data.color, rustgis_engine::data::raster::ColorHint::Palette(_));
        let kind = style.renderer.code();
        big_menu(ui, app, raster_renderer_icon(&style.renderer), "Symbology", "Choose how the raster is drawn.", move |ui, app| {
            for (code, label) in RasterRenderer::KINDS {
                let ok = match code {
                    "rgb" => n >= 3,
                    "unique" => has_categories,
                    "colormap" => palette,
                    _ => true,
                };
                let checked = kind == code;
                if ui.add_enabled(ok, egui::Button::selectable(checked, tl!(label))).clicked() {
                    let _ = app.run("raster.symbology", json!({"type": code}));
                    ui.close();
                }
            }
            ui.separator();
            if ui.button(tl!("Symbology Pane…")).clicked() {
                app.ui.open_right(RightPane::Symbology);
                ui.close();
            }
        });
        let current = style.stretch;
        big_menu(ui, app, "stretch", "Stretch\nType", "Choose a contrast stretch for the raster layer.", move |ui, app| {
            for (code, label) in Stretch::KINDS {
                if ui.add(egui::Button::selectable(current.code() == code, tl!(label))).clicked() {
                    let _ = app.run("raster.stretch", json!({"type": code}));
                    ui.close();
                }
            }
        });
        col(ui, |ui| {
            small(
                ui,
                app,
                "dra",
                "DRA",
                "Dynamic range adjustment: stretch using the pixels in view.",
                "raster.dra",
                Value::Null,
                style.dra,
            );
            let res = style.resampling;
            small_menu(ui, app, "resample", "Resampling", "Choose a resampling type for the raster layer.", move |ui, app| {
                for r in Resampling::ALL {
                    if ui.add(egui::Button::selectable(r == res, tl!(r.label()))).clicked() {
                        let _ = app.run("raster.resampling", json!({"type": format!("{r:?}")}));
                        ui.close();
                    }
                }
            });
            let bands: Vec<String> = data.bands.iter().map(|b| b.name.clone()).collect();
            let cur = match style.renderer {
                RasterRenderer::Rgb { red, green, blue } => Some([red, green, blue]),
                _ => None,
            };
            small_menu(ui, app, "bands", "Band Combination", "Choose a band combination to display the raster layer.", move |ui, app| {
                if bands.len() < 3 {
                    ui.label(tl!("Needs three or more bands."));
                    return;
                }
                if ui.add(egui::Button::selectable(cur == Some([0, 1, 2]), tl!("Natural Color (1, 2, 3)"))).clicked() {
                    let _ = app.run("raster.bands", json!({"preset": "natural"}));
                    ui.close();
                }
                if bands.len() >= 4 && ui.add(egui::Button::selectable(cur == Some([3, 0, 1]), tl!("Color Infrared (4, 1, 2)"))).clicked() {
                    let _ = app.run("raster.bands", json!({"preset": "colorInfrared"}));
                    ui.close();
                }
                ui.separator();
                for (key, label, slot) in [("red", "Red", 0usize), ("green", "Green", 1), ("blue", "Blue", 2)] {
                    ui.menu_button(tl!(label), |ui| {
                        for (i, b) in bands.iter().enumerate() {
                            if ui
                                .add(egui::Button::selectable(
                                    cur.and_then(|c| c.get(slot).copied()) == Some(i),
                                    crate::panes::band_name(b),
                                ))
                                .clicked()
                            {
                                let _ = app.run("raster.bands", json!({ key: i + 1 }));
                                ui.close();
                            }
                        }
                    });
                }
            });
        });
    });
    group(ui, "Enhancement", |ui| {
        ui.vertical(|ui| {
            ui.add_space(2.0);
            for (key, label, icon, value, range, speed) in [
                ("brightness", "Brightness", "brightness", style.brightness, -100.0..=100.0, 0.5),
                ("contrast", "Contrast", "contrast", style.contrast, -100.0..=100.0, 0.5),
                ("gamma", "Gamma", "gamma", style.gamma, 0.1..=10.0, 0.01),
            ] {
                ui.horizontal(|ui| {
                    let t = Tokens::get(ui.ctx());
                    let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                    icons::paint(ui.painter(), r, icon, t.icon, t.accent);
                    let input_label = ui.label(egui::RichText::new(tl!(label)).size(11.5));
                    ui.add_space((70.0 - text_w(ui, tl!(label), regular(11.5))).max(0.0));
                    let mut v = value;
                    let resp = ui.add(egui::DragValue::new(&mut v).range(range).speed(speed).max_decimals(2)).labelled_by(input_label.id);
                    live_drag(
                        app,
                        ui,
                        &resp,
                        value,
                        v,
                        |app, x| {
                            let rl =
                                app.session.active_layer.and_then(|id| app.session.map_mut().layer_mut(id)).and_then(|l| l.raster.as_mut());
                            if let Some(rl) = rl {
                                match key {
                                    "brightness" => rl.style.brightness = x,
                                    "contrast" => rl.style.contrast = x,
                                    _ => rl.style.gamma = x,
                                }
                            }
                        },
                        |app, x| {
                            let _ = app.run("raster.enhance", json!({ key: x }));
                        },
                    );
                });
            }
        });
        big(
            ui,
            app,
            "undo",
            "Reset",
            "Reset brightness, contrast and gamma.",
            "raster.enhance",
            json!({"brightness": 0, "contrast": 0, "gamma": 1}),
            false,
            false,
        );
    });
    layer_group(app, ui, &layer);
}

fn raster_renderer_icon(r: &RasterRenderer) -> &'static str {
    match r {
        RasterRenderer::Stretch { .. } => "stretch",
        RasterRenderer::Rgb { .. } => "rgb",
        RasterRenderer::Classify { .. } => "classify",
        RasterRenderer::UniqueValues { .. } => "unique",
        RasterRenderer::Colormap => "raster_layer",
        RasterRenderer::ShadedRelief { .. } => "relief",
    }
}

fn raster_data_groups(app: &mut RustGisApp, ui: &mut Ui) {
    group(ui, "Information", |ui| {
        big(
            ui,
            app,
            "identify",
            "Raster\nInformation",
            "Size, bands, statistics and coordinate system.",
            "ui.pane.details",
            Value::Null,
            app.ui.right == Some(RightPane::Details),
            false,
        );
    });
    group(ui, "Export", |ui| {
        big(ui, app, "export_layer", "Export\nRaster", "Save the raster as a GeoTIFF.", "ui.exportRaster", Value::Null, false, false);
    });
}

fn renderer_icon(r: &Renderer) -> &'static str {
    match r {
        Renderer::Single(_) => "single",
        Renderer::Unique { .. } => "unique",
        Renderer::Graduated { .. } => "graduated",
        Renderer::Proportional { .. } => "proportional",
    }
}

/// A ribbon size preview belongs to the layer where the gesture began, even if selection moves.
#[derive(Clone, Default)]
struct LabelSizeDrag {
    layer: u32,
    before: f32,
    // Keep the widget's precision for command recording; layer storage is f32.
    size: f64,
}

/// Retains the starting size and layer while applying each frame's preview.
fn label_size_drag(app: &mut RustGisApp, ui: &Ui, layer: &rustgis_engine::map::Layer, size: f64) {
    let key = egui::Id::unique("ribbon_label_size_drag");
    let pending = ui.data_mut(|d| d.remove_temp::<LabelSizeDrag>(key));
    let before = match pending {
        Some(drag) if drag.layer == layer.id => drag.before,
        Some(drag) => {
            apply_label_size_drag(app, drag);
            layer.labels.default.size
        }
        None => layer.labels.default.size,
    };
    ui.data_mut(|d| d.insert_temp(key, LabelSizeDrag { layer: layer.id, before, size }));
    if let Some(layer) = app.session.map_mut().layer_mut(layer.id) {
        layer.labels.default.size = size as f32;
    }
    app.session.revision = app.session.revision.wrapping_add(1);
}

/// Called after the panes even if selection is cleared or the ribbon is hidden mid-gesture.
pub(crate) fn finish_label_size_drag(app: &mut RustGisApp, ui: &Ui) {
    if ui.input(|i| i.pointer.primary_down() && i.focused) {
        return;
    }
    if let Some(drag) = ui.data_mut(|d| d.remove_temp::<LabelSizeDrag>(egui::Id::unique("ribbon_label_size_drag"))) {
        apply_label_size_drag(app, drag);
    }
}

/// Restores the original size before committing on the owning layer, so Undo captures that size.
fn apply_label_size_drag(app: &mut RustGisApp, drag: LabelSizeDrag) {
    let Some(layer) = app.session.map_mut().layer_mut(drag.layer) else { return };
    layer.labels.default.size = drag.before;
    if drag.before != drag.size as f32 {
        let selected = app.session.active_layer.replace(drag.layer);
        let _ = app.run("labels.style", json!({"size": drag.size}));
        app.session.active_layer = selected;
    }
}

fn label_groups(app: &mut RustGisApp, ui: &mut Ui) {
    let Some(layer) = app.session.active_layer.and_then(|id| app.session.map().layer(id)).cloned() else { return };
    group(ui, "Labels", |ui| {
        let on = layer.labels.enabled;
        big(ui, app, if on { "label" } else { "label_off" }, "Label", "Turn labels on or off.", "labels.enable", Value::Null, on, false);
        big(
            ui,
            app,
            "label_properties",
            "Properties",
            "Every label setting of this layer, in the Labeling pane.",
            "ui.pane.labeling",
            Value::Null,
            app.ui.right == Some(RightPane::Labeling),
            false,
        );
        let cur = layer.labels.default.placement;
        ui.vertical(|ui| {
            ui.add_space(8.0);
            let fields: Vec<String> = layer.dataset.fields.iter().map(|f| f.name.clone()).collect();
            if let Some(f) = crate::widgets::ribbon_combo(ui, "label_field", "Field", &layer.labels.default.field, &fields, 140.0) {
                let _ = app.run("labels.field", json!({"field": f}));
            }
        });
        big_menu(ui, app, "placement", "Placement", "Where labels sit relative to their features.", |ui, app| {
            for pl in rustgis_engine::map::LabelPlacement::ALL {
                if ui.radio(cur == pl, tl!(pl.label())).clicked() {
                    let _ = app.run("labels.placement", json!({"position": pl.code()}));
                    ui.close();
                }
            }
        });
    });
    group(ui, "Label Text", |ui| {
        col(ui, |ui| {
            // The whole range `labels.style` accepts (#457), so a larger label isn't cut back.
            let mut size = f64::from(layer.labels.default.size);
            ui.horizontal(|ui| {
                let input_label = ui.label(egui::RichText::new(tl!("Size")).size(11.5));
                let range = rustgis_engine::map::symbology::LABEL_SIZE;
                // Another layer gets a distinct widget, so an in-flight drag cannot move to it.
                ui.push_id(("label_size", layer.id), |ui| {
                    if ui.add(egui::DragValue::new(&mut size).range(range).speed(0.1).suffix(" pt")).labelled_by(input_label.id).changed() {
                        label_size_drag(app, ui, &layer, size);
                    }
                });
            });
            let mut c = crate::theme::c32(layer.labels.default.color);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(tl!("Color")).size(11.5));
                ui.push_id(("label_color", layer.id), |ui| {
                    if crate::widgets::color_button(ui, &mut c).changed() {
                        crate::labelpane::color_drag(app, ui, &layer, None, crate::theme::rgba(c));
                    }
                });
            });
        });
        col(ui, |ui| {
            small(
                ui,
                app,
                "label",
                "Bold",
                "Bold label text.",
                "labels.style",
                json!({"bold": !layer.labels.default.bold}),
                layer.labels.default.bold,
            );
            small(
                ui,
                app,
                "single",
                "Halo",
                "Draw a halo around label text.",
                "labels.style",
                json!({"halo": !layer.labels.default.halo}),
                layer.labels.default.halo,
            );
        });
    });
    group(ui, "Label Range", |ui| {
        ui.vertical(|ui| {
            ui.add_space(6.0);
            for (label, cur, key) in
                [("Out Beyond", layer.labels.default.min_scale, "min"), ("In Beyond", layer.labels.default.max_scale, "max")]
            {
                ui.horizontal(|ui| {
                    let input_label = ui.label(egui::RichText::new(tl!(label)).size(11.5));
                    egui::ComboBox::from_id_salt(format!("label_scale_{key}"))
                        .selected_text(scale_label(cur))
                        .width(118.0)
                        .show_ui(ui, |ui| {
                            let mut choices: Vec<Option<f64>> = vec![None];
                            choices.extend(SCALES.iter().map(|s| Some(*s)));
                            for c in choices {
                                if ui.selectable_label(c == cur, scale_label(c)).clicked() {
                                    let _ = app.run("labels.scaleRange", json!({key: c}));
                                }
                            }
                        })
                        .response
                        .labelled_by(input_label.id);
                });
            }
        });
        col(ui, |ui| {
            for (label, cur, key, desc) in [
                ("Closest", layer.labels.default.closest, "closest", "Hide labels when the viewer is closer than this."),
                ("Farthest", layer.labels.default.farthest, "farthest", "Hide labels when the viewer is farther than this."),
            ] {
                small_menu(ui, app, "eye", label, desc, |ui, app| {
                    for d in [None, Some(1_000.0), Some(10_000.0), Some(100_000.0), Some(1_000_000.0), Some(10_000_000.0)] {
                        let text = d
                            .map(|d| crate::measure_units::distance(d, app.ui.measurement_imperial))
                            .unwrap_or_else(|| tl!("<None>").to_string());
                        if ui.radio(cur == d, text).clicked() {
                            let _ = app.run("labels.distanceRange", json!({key: d}));
                            ui.close();
                        }
                    }
                });
            }
            small(
                ui,
                app,
                "scale_label",
                "Clear Limits",
                "Draw labels at every scale and distance.",
                "labels.scaleRange",
                json!({"clear": true}),
                false,
            );
        });
    });
}

fn feature_data_groups(app: &mut RustGisApp, ui: &mut Ui) {
    group(ui, "Table", |ui| {
        big(ui, app, "table", "Attribute\nTable", "Open the layer's attribute table.", "table.open", Value::Null, false, false);
        let has = active_layer(app).is_some_and(|l| l.definition_query.is_some());
        big(
            ui,
            app,
            "def_query",
            "Definition\nQuery",
            "Limit which features draw with a SQL expression.",
            "ui.dialog",
            json!({"name": "definitionQuery"}),
            has,
            false,
        );
    });
    group(ui, "Selection", |ui| {
        col(ui, |ui| {
            small(ui, app, "select", "All", "Select every feature.", "select.all", Value::Null, false);
            small(ui, app, "select_loc", "Switch", "Invert the selection.", "select.switch", Value::Null, false);
            small(ui, app, "clear_sel", "Clear", "Clear the selection.", "select.clear", Value::Null, false);
        });
        big(
            ui,
            app,
            "add_data",
            "Layer From\nSelection",
            "Make a new layer of the selected features.",
            "select.layerFromSelection",
            Value::Null,
            false,
            false,
        );
    });
    group(ui, "Fields", |ui| {
        big(ui, app, "field", "Add Field", "Add a field.", "ui.dialog", json!({"name": "addField"}), false, false);
        big(ui, app, "calculate", "Calculate", "Calculate field values.", "ui.dialog", json!({"name": "calculateField"}), false, false);
        big(
            ui,
            app,
            "table",
            "Data\nDesign",
            "Subtypes, domains, attribute rules, contingent values and relationships.",
            "ui.pane.design",
            Value::Null,
            app.ui.right == Some(RightPane::Design),
            false,
        );
        col(ui, |ui| {
            let on = active_layer(app).is_some_and(|l| l.design.archive.is_some());
            small(ui, app, "history", "Archive", "Keep a history of every edit to the layer.", "data.archive", json!({"on": !on}), on);
            small_menu(ui, app, "select_loc", "Related Data", "Select the rows related to the selection.", |ui, app| {
                let rels: Vec<String> = app.session.map().editing.relationships.iter().map(|r| r.name.clone()).collect();
                if rels.is_empty() {
                    ui.label(egui::RichText::new(tl!("No relationships yet")).weak());
                }
                for name in rels {
                    menu_item(ui, app, "select_loc", &name, "data.related", json!({"name": name}));
                }
                ui.separator();
                menu_item(ui, app, "add_data", "New Relationship…", "ui.pane.design", Value::Null);
            });
        });
    });
    group(ui, "Export", |ui| {
        big(
            ui,
            app,
            "export_layer",
            "Export\nFeatures",
            "Export the selected layer to GeoJSON or CSV.",
            "ui.exportLayer",
            Value::Null,
            false,
            false,
        );
        col(ui, |ui| {
            small(
                ui,
                app,
                "export_layer",
                "Export To 3D…",
                "Export the layer as a 3D model (OBJ).",
                "ui.saveItem",
                json!({"kind": "obj"}),
                false,
            );
        });
    });
}

/// A language's own name, or its English name when no installed font can draw it (CJK names on the
/// web before a CJK font has been downloaded).
fn shown_name(ui: &Ui, l: crate::i18n::Lang) -> &'static str {
    if ui.ctx().fonts_mut(|f| f.has_glyphs(&regular(12.5), l.name())) { l.name() } else { l.english() }
}

/// Whose view of disputed borders the built-in countries and world outline show (Settings › Language).
fn worldview_menu(app: &mut RustGisApp, ui: &mut Ui) {
    use rustgis_engine::sample::Worldview;
    let name = |w: Worldview| match w {
        Worldview::Default => tl!("Default"),
        Worldview::China => tl!("China"),
    };
    let auto = app.ui.worldview == crate::i18n::AUTO;
    let current = rustgis_engine::sample::worldview();
    let label = if auto { format!("{} ({})", tl!("Automatic"), name(current)) } else { name(current).to_string() };
    let label = format!("{}\n{label}", tl!("Worldview"));
    big_menu(ui, app, "globe_view", &label, "Whose borders the sample countries and the basemap outline show.", move |ui, app| {
        if ui.selectable_label(auto, tl!("Automatic (system time zone and region)")).clicked() {
            let _ = app.run("ui.worldview", json!({"worldview": crate::i18n::AUTO}));
            ui.close();
        }
        ui.separator();
        for w in [Worldview::Default, Worldview::China] {
            if ui.selectable_label(!auto && w == current, name(w)).clicked() {
                let _ = app.run("ui.worldview", json!({"worldview": w.code()}));
                ui.close();
            }
        }
    });
}

/// The interface language picker (Settings › Language).
fn language_menu(app: &mut RustGisApp, ui: &mut Ui) {
    let current = app.chosen_lang();
    let auto = app.ui.language == crate::i18n::AUTO;
    let label = if auto { format!("{} ({})", tl!("Automatic"), shown_name(ui, current)) } else { shown_name(ui, current).to_string() };
    big_menu(ui, app, "globe_lang", &label, "Choose the interface language.", move |ui, app| {
        if ui.selectable_label(auto, tl!("Automatic (system language)")).clicked() {
            let _ = app.run("ui.language", json!({"code": crate::i18n::AUTO}));
            ui.close();
        }
        ui.separator();
        for l in crate::i18n::Lang::all() {
            let sel = !auto && l == current;
            if ui.selectable_label(sel, shown_name(ui, l)).on_hover_text(l.english()).clicked() {
                let _ = app.run("ui.language", json!({"code": l.code()}));
                ui.close();
            }
        }
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;

    /// Isolates the label controls; the interrupted-gesture test below checks full-frame wiring.
    fn label_harness() -> egui_kittest::Harness<'static, RustGisApp> {
        let mut app = RustGisApp::with_sample();
        app.session.run("layer.select", &json!({"layer": "Cities"})).unwrap();
        app.ws.recording = Some(Vec::new());
        let mut fonts = false;
        let mut h = egui_kittest::Harness::builder().with_size(vec2(1600.0, 700.0)).build_ui_state(
            move |ui, app: &mut RustGisApp| {
                if !fonts {
                    crate::theme::install_fonts(ui.ctx(), &[], None);
                    fonts = true;
                    return;
                }
                if !app.ui.ribbon_minimized {
                    ui.horizontal(|ui| label_groups(app, ui));
                }
                finish_label_size_drag(app, ui);
                crate::labelpane::finish_color_drag(app, ui);
            },
            app,
        );
        h.run();
        h
    }

    #[test]
    fn ribbon_label_size_drag_is_one_undo_and_recorded_step() {
        let mut h = label_harness();
        let id = h.state().session.active_layer.unwrap();
        let size = |app: &RustGisApp| app.session.map().layer(id).unwrap().labels.default.size;
        let before = size(h.state());
        let undo = h.state().session.undo_depth();
        let at = h.get_by_role(egui::accesskit::Role::SpinButton).rect().center();
        h.hover_at(at);
        h.drag_at(at);
        h.step();
        for dx in [16.0, 32.0, 48.0] {
            h.hover_at(at + vec2(dx, 0.0));
            h.step();
            assert!(size(h.state()) > before, "the map previews the size while dragging");
            assert_eq!(h.state().session.undo_depth(), undo, "a drag does not run a command each frame");
            assert!(h.state().ws.recording.as_ref().unwrap().is_empty());
        }
        let after = size(h.state());
        h.drop_at(at + vec2(48.0, 0.0));
        h.run();
        assert_eq!(h.state().session.undo_depth(), undo + 1);
        let steps = h.state().ws.recording.as_ref().unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].command, "labels.style");
        assert_eq!(steps[0].params["layer"], "Cities");
        assert_eq!(size(h.state()), after);
        h.state_mut().run("edit.undo", json!(null)).unwrap();
        assert_eq!(size(h.state()), before);
        h.state_mut().run("edit.redo", json!(null)).unwrap();
        assert_eq!(size(h.state()), after);
    }

    #[test]
    fn ribbon_label_size_drag_keeps_its_layer_when_selection_changes() {
        for clear_selection in [false, true] {
            let mut h = label_harness();
            let cities = h.state().session.active_layer.unwrap();
            let countries = h.state().session.map().layers.iter().find(|l| l.name == "Countries").unwrap().id;
            let size = |app: &RustGisApp, id| app.session.map().layer(id).unwrap().labels.default.size;
            let before = size(h.state(), cities);
            let countries_before = size(h.state(), countries);
            let undo = h.state().session.undo_depth();
            let at = h.get_by_role(egui::accesskit::Role::SpinButton).rect().center();
            h.hover_at(at);
            h.drag_at(at);
            h.step();
            h.hover_at(at + vec2(32.0, 0.0));
            h.step();
            let after = size(h.state(), cities);
            assert!(after > before);
            // Selection may also change through the Assistant or relay while the pointer is held.
            let selected = if clear_selection { None } else { Some(countries) };
            h.state_mut().session.active_layer = selected;
            h.step();
            h.drop_at(at + vec2(32.0, 0.0));
            h.run();
            assert_eq!(size(h.state(), countries), countries_before);
            assert_eq!(size(h.state(), cities), after);
            assert_eq!(h.state().session.active_layer, selected);
            assert_eq!(h.state().session.undo_depth(), undo + 1);
            let steps = h.state().ws.recording.as_ref().unwrap();
            assert_eq!(steps.len(), 1);
            assert_eq!(steps[0].params["layer"], "Cities");
            h.state_mut().run("edit.undo", json!(null)).unwrap();
            assert_eq!(size(h.state(), cities), before);
            assert_eq!(size(h.state(), countries), countries_before);
            h.state_mut().run("edit.redo", json!(null)).unwrap();
            assert_eq!(size(h.state(), cities), after);
        }
    }

    /// Exercises the app frame so removing the pane finalizer breaks gesture completion.
    #[test]
    fn ribbon_label_size_drag_finishes_when_hidden_or_unfocused() {
        for hide_ribbon in [false, true] {
            let mut app = RustGisApp::with_sample();
            app.session.run("layer.select", &json!({"layer": "Cities"})).unwrap();
            app.ui.tab = "Style".into();
            let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).with_max_steps(64).build_ui_state(
                |ui, app: &mut RustGisApp| {
                    let ctx = ui.ctx().clone();
                    app.logic(&ctx);
                    app.ui(ui);
                },
                app,
            );
            h.run();
            for _ in 0..2 {
                h.get_by_label("Scroll the ribbon right").click();
                h.run();
            }
            h.state_mut().ws.recording = Some(Vec::new());
            let id = h.state().session.active_layer.unwrap();
            let size = |app: &RustGisApp| app.session.map().layer(id).unwrap().labels.default.size;
            let before = size(h.state());
            let undo = h.state().session.undo_depth();
            // Text Symbol follows the Drawing group's numeric controls in the Style ribbon.
            let at = h.query_all_by_role(egui::accesskit::Role::SpinButton).last().unwrap().rect().center();
            h.hover_at(at);
            h.drag_at(at);
            h.step();
            h.hover_at(at + vec2(32.0, 0.0));
            h.step();
            let after = size(h.state());
            assert!(after > before);
            if hide_ribbon {
                h.state_mut().ui.ribbon_minimized = true;
                h.step();
                h.drop_at(at + vec2(32.0, 0.0));
            } else {
                h.input_mut().focused = false;
                h.event(egui::Event::WindowFocused(false));
            }
            h.run();
            assert_eq!(size(h.state()), after);
            assert_eq!(h.state().session.undo_depth(), undo + 1);
            assert_eq!(h.state().ws.recording.as_ref().unwrap().len(), 1);
            h.state_mut().run("edit.undo", json!(null)).unwrap();
            assert_eq!(size(h.state()), before);
        }
    }

    #[test]
    fn ribbon_label_colour_drag_is_one_undo_and_recorded_step() {
        let mut h = label_harness();
        let id = h.state().session.active_layer.unwrap();
        let color = |app: &RustGisApp| app.session.map().layer(id).unwrap().labels.default.color;
        let before = color(h.state());
        let undo = h.state().session.undo_depth();
        h.get_by_role(egui::accesskit::Role::ColorWell).click();
        h.run();
        // The ribbon size precedes the colour picker's R/G/B/A numeric controls.
        let at = h.query_all_by_role(egui::accesskit::Role::SpinButton).nth(1).unwrap().rect().center();
        h.hover_at(at);
        h.drag_at(at);
        h.step();
        for dx in [16.0, 32.0, 48.0] {
            h.hover_at(at + vec2(dx, 0.0));
            h.step();
            assert_ne!(color(h.state()), before, "the map previews the colour while dragging");
            assert_eq!(h.state().session.undo_depth(), undo, "the picker does not run a command each frame");
            assert!(h.state().ws.recording.as_ref().unwrap().is_empty());
        }
        let after = color(h.state());
        h.drop_at(at + vec2(48.0, 0.0));
        h.run();
        assert_eq!(h.state().session.undo_depth(), undo + 1);
        let steps = h.state().ws.recording.as_ref().unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].command, "labels.style");
        assert_eq!(steps[0].params["layer"], "Cities");
        assert_eq!(color(h.state()), after);
        h.state_mut().run("edit.undo", json!(null)).unwrap();
        assert_eq!(color(h.state()), before);
        h.state_mut().run("edit.redo", json!(null)).unwrap();
        assert_eq!(color(h.state()), after);
    }

    #[test]
    fn ribbon_label_colour_drag_keeps_its_layer_when_selection_changes() {
        let mut h = label_harness();
        let cities = h.state().session.active_layer.unwrap();
        let countries = h.state().session.map().layers.iter().find(|l| l.name == "Countries").unwrap().id;
        let color = |app: &RustGisApp, id| app.session.map().layer(id).unwrap().labels.default.color;
        let before = color(h.state(), cities);
        let countries_before = color(h.state(), countries);
        let undo = h.state().session.undo_depth();
        h.get_by_role(egui::accesskit::Role::ColorWell).click();
        h.run();
        let at = h.query_all_by_role(egui::accesskit::Role::SpinButton).nth(1).unwrap().rect().center();
        h.hover_at(at);
        h.drag_at(at);
        h.step();
        h.hover_at(at + vec2(32.0, 0.0));
        h.step();
        let after = color(h.state(), cities);
        assert_ne!(after, before);
        h.state_mut().session.active_layer = Some(countries);
        h.step();
        h.hover_at(at + vec2(64.0, 0.0));
        h.step();
        h.drop_at(at + vec2(64.0, 0.0));
        h.run();
        assert_eq!(color(h.state(), countries), countries_before);
        assert_eq!(color(h.state(), cities), after);
        assert_eq!(h.state().session.active_layer, Some(countries));
        assert_eq!(h.state().session.undo_depth(), undo + 1);
        let steps = h.state().ws.recording.as_ref().unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].params["layer"], "Cities");
        h.state_mut().run("edit.undo", json!(null)).unwrap();
        assert_eq!(color(h.state(), cities), before);
        assert_eq!(color(h.state(), countries), countries_before);
        h.state_mut().run("edit.redo", json!(null)).unwrap();
        assert_eq!(color(h.state(), cities), after);
    }

    #[test]
    fn ribbon_label_size_text_input_preserves_decimal_command_value() {
        let mut h = label_harness();
        let id = h.state().session.active_layer.unwrap();
        let before = h.state().session.map().layer(id).unwrap().labels.default.size;
        let undo = h.state().session.undo_depth();
        h.get_by_role(egui::accesskit::Role::SpinButton).focus();
        h.run();
        // Inserts the complete value in one text event; per-keystroke undo grouping is separate.
        h.get_by_role(egui::accesskit::Role::SpinButton).type_text("14.3");
        h.key_press(egui::Key::Enter);
        h.run();
        assert_eq!(h.state().session.map().layer(id).unwrap().labels.default.size, 14.3_f32);
        assert_eq!(h.state().session.undo_depth(), undo + 1);
        assert_eq!(h.state().ws.recording.as_ref().unwrap().len(), 1);
        assert_eq!(h.state().ws.recording.as_ref().unwrap()[0].params["size"], json!(14.3));
        h.state_mut().run("edit.undo", json!(null)).unwrap();
        assert_eq!(h.state().session.map().layer(id).unwrap().labels.default.size, before);
    }

    /// Every publication profile has its literal name in the menu, the engine's label word for word
    /// (the test runs in English), so neither can be renamed without the other.
    #[test]
    fn every_publication_profile_is_named_by_a_literal() {
        for p in rustgis_engine::cmd::layout::PUBLICATION_PROFILES {
            assert_eq!(publication_label(p.id), Some(p.label), "{}", p.id);
        }
    }

    #[test]
    fn saved_hidden_tabs_never_hide_the_fixed_tabs() {
        let saved = ["Settings", "Help", "Edit", "Share", "Edit"].iter().map(|t| t.to_string()).collect();
        assert_eq!(current_hidden_tabs(saved), vec!["Edit".to_string(), "Publish".to_string()]);
    }
}
