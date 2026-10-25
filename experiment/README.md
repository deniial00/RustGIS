# Ribbon experiment

Material for the community discussion on how the RustGIS ribbon should be organised. Nothing here
is meant to be merged as is.

## Files

| File | What it is |
|---|---|
| [ribbon-audit.md](ribbon-audit.md) | Inventory of the ribbon as it is: every tab, group and dropdown, what each control opens, estimated usage, and where things repeat |
| [ribbon-rulebook.md](ribbon-rulebook.md) | A draft rule book: which features get a ribbon control, and which kind |
| [ribbon-proposal.md](ribbon-proposal.md) | One possible new layout that applies the rule book |
| [ribbon-arcgis.md](ribbon-arcgis.md) | How ArcGIS Pro organises its ribbon, with sources |
| [ribbon-qgis.md](ribbon-qgis.md) | How QGIS and its experimental Ribbon Toolbar plugin do it, with sources |

## The prototype

This branch also contains a prototype of `ribbon-proposal.md` (the commit before the one adding
this folder). To try it:

```sh
cargo run --release -p rustgis
```

Notes:

- The audit and the prototype are based on `main` at `c281e2a`, before #855 added the Select
  split button (Polygon, Lasso, Radius). In the prototype, Select is a plain button.
- At 1366 px wide, the View, Analyze and Style (polygon layer) tabs still need the scroll arrows.
- The usage ratings in the audit are estimates, not measurements.

## Screenshots of the prototype

Headless shots of the ribbon strip (`cargo run --release -p rustgis-ui-egui --example ui_shot --
experiment/screenshots/script.jsonl`), 1366 px wide at 2×, World Atlas sample. The Style shots
have Countries (polygons) or Cities (points) selected. The `wide-*` shots (from `wide.jsonl`) are
2600 px wide and show the tabs that don't fit at 1366 px in full.

| Tab | Screenshot |
|---|---|
| Home | ![Home](screenshots/home.png) |
| Data | ![Data](screenshots/data.png) |
| Style, polygon layer | ![Style, polygon layer](screenshots/style-polygon.png) |
| Style, point layer | ![Style, point layer](screenshots/style-point.png) |
| Edit | ![Edit](screenshots/edit.png) |
| Analyze | ![Analyze](screenshots/analyze.png) |
| View | ![View](screenshots/view.png) |
| View, Panes menu open | ![View with the Panes menu open](screenshots/view-panes-menu.png) |
| Publish | ![Publish](screenshots/publish.png) |
| Publish, layout open | ![Publish with a layout open](screenshots/publish-layout-open.png) |
| Layout | ![Layout](screenshots/layout.png) |
| Plugins | ![Plugins](screenshots/plugins.png) |
| Settings | ![Settings](screenshots/settings.png) |
| Help | ![Help](screenshots/help.png) |
| Backstage, Start page | ![Backstage Start page](screenshots/backstage-start.png) |
| Analyze, full width | ![Analyze at 2600 px](screenshots/wide-analyze.png) |
| View, full width | ![View at 2600 px](screenshots/wide-view.png) |
| Style polygon, full width | ![Style, polygon layer, at 2600 px](screenshots/wide-style-polygon.png) |
| Style point, full width | ![Style, point layer, at 2600 px](screenshots/wide-style-point.png) |
