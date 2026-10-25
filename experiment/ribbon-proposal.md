# Proposed ribbon structure for RustGIS

A concrete layout for discussion. It applies the rules in `ribbon-rulebook.md` to the controls
listed in `ribbon-audit.md`. No feature is removed: every control that leaves the ribbon is listed
in section 4 with where it went.

## 1. Overview

```
FORM   [B] big button   [S] small button   ⑂ split button (fixed default | menu)   ▾ dropdown
       [x] toggle   [D] dialog   [P] pane   [W] window   [F] file picker   [T] map tool
NEW    ★ new in this proposal (control, group or tab)   ⇄ deliberate mirror (rule 2)
```

| Tab | Controls now | Proposed | Main changes |
|---|---|---|---|
| Home | 34 | 25 | Project group cut to New Map and New Layout; Select and Add Data become split buttons |
| Data | 24 | 15 | Data Design is one button; Feature Cache goes to Settings |
| Style (polygon layer) | 36 | 35 | Extrusion, faces and lighting are one 3D group; single-control groups merged (mostly regrouping) |
| Edit | 33 | 26 | Editing pane is one button (it also replaces Modify); the 5 polygon and 2 text tools become 2 split buttons |
| Analyze | 49 | 28 | 8 imagery groups become one Imagery group with 3 controls; Portal and Selection go |
| View | 53 | 33 | Windows keeps 4 general panes plus a Panes ▾ menu; Effects and Scene become dropdowns |
| Publish | 44 | 18 | Package, Share As and Save As become one split button each; Thumbnail goes to the backstage |
| Layout ★ (contextual) | ~30 (inside Publish) | 22 | Layout groups move to their own tab, shown while a layout is open |
| Plugins / Settings / Help | 3 / 7 / 6 | 2 / 12 / 5 | Browse Plugins goes (it is a tab of the Plugin Manager); Settings gets a Display group |
| **Total without a layout open** | **~290** | **~200** | |
| **Total with a layout open** | **~320** | **~220** | |

Counts are by hand from `ribbon.rs`, ±5, and exclude plugin buttons.

## 2. Tabs

### Project button (backstage)

```
Navigation      Back · Start · New · Open [F] · Save · Save As [F] · About          (unchanged)
Start page      New Project:  Map · World Atlas · From Template… [F] ★ · Open… [F]
                              · QGIS Project… [F] · Import Maps… [F] ★
                Current Project: Name · Location · Maps · Layers
                              · Thumbnail ★: Create From View · Import Image… [F]
                              · Map Thumbnail ★: Create From View · Import Image… [F]
```

From Template… comes from Home › Templates ▾ › Open Custom Template… Import Maps… comes from
Home › Project › Import. Both thumbnails come from Publish › Thumbnail.

### Home: where work starts and the everyday map tools

```
Project      New Map [B] · New Layout ⑂ [B]  (Letter | page sizes · Publication Layout ▸ · Import Layout… [F])
Navigate     Pan [B][T] · Bookmarks ▾ [B]
             Full Extent · Zoom In · Zoom Out [S]
             Previous · Next · Zoom Box [T] [S]
             Go To XY [D] · Find Place [D] · Directions [P] [S]
Add          Basemap ▾ [B] (gallery, unchanged)
             Add Data ⑂ [B]  (Data… [F] | Delimited Text… · Geocode Addresses… · From URL… · From Service…
                              · Zarr… · Database… · S3… · Connections… · New Connection ▸
                              ─ Graphics Layer · Layer Templates ▸ ★ · Add Folder… ★ ─ Sample data)
             Web Data ▾ [B]  (unchanged; see open question 3)
Selection    Select ⑂ [B][T]  (Box | Polygon · Lasso · Radius; exists on main since #855)
             By Attributes [B][D] · By Location [B][P]
             Clear · Zoom To · Attributes [S]
Inspect      Measure Distance · Measure Area [S][T] · Infographics [S][P]   (units → Settings › Display)
```

### Data: the selected layer's records (contextual, as today)

```
Table        Attribute Table [B] · Definition Query [B][D]
Selection ⇄  All · Switch · Clear [S] · Layer From Selection [B]
Fields       Add Field [B][D] · Calculate [B][D] · Data Design [B][P] ★ (one button for Subtypes, Domains,
             Attribute Rules, Contingent Values, relationships)
             Archive [S][x] · Related Data ▾ [S]
Export       Export Features [B][D] · Export To 3D… [S][F] ★
             raster layer: Export Raster [B][F]
Offline      Download Map [B] · Offline Areas ▾ [S] (Sync ▸ · Open ▸ · Remove ▸)
raster:      Raster Information [B][P] takes the place of Fields
```

### Style: the selected layer's drawing (contextual, as today)

```
Visibility   Out Beyond ▾ · In Beyond ▾ · Clear Limits · Transparency slider        (2 groups merged)
Drawing      Symbology [B][P] · Primary Symbology ▾ [B] · Color · Outline · Size / No Fill
Effects      Masking ▾ · Display Filters ▾ · Aggregation ▾ (points)
3D ★         Extrusion ▾ [B] · Factor · Face Culling [x] · Lighting [x] · Cast Shadow [x]
             (polygons; the two lighting toggles become one)
Layer        Zoom To Layer [B] · Visible [x] · Remove · Draw Priority ▾ ★ (Raise · Lower, from View › Scene)
Labels       Label [B][x] · Properties [B][P] · Field · Placement ▾                   (3 groups merged)
Label Text   Size · Color · Bold [x] · Halo [x]
Label Range  Out/In Beyond ▾ · Closest ▾ · Farthest ▾ · Clear Limits
Styles       Styles ▾ [B]
raster:      Visibility · Compare (Swipe) · Rendering · Enhancement · Raster Effects · Layer (unchanged)
```

### Edit

```
Manage Edits Edit [B][x] · Save [B] ⇄
             Undo · Redo [S] · Editing Pane [S][P] ★ (replaces Status, Settings, Templates, Topology Options)
             Validate Topology [S]
Selection ⇄  Select ⑂ [B][T] · Clear [S]
Options      Snapping [B][x] · Elevation ▾ [S]
Features     Create [B][T] · Edit Vertices [B][T] · Delete [B]
             Editor Tools ▾ [B] (Merge · Explode · Flip Line · Rotate 90° · Scale ×2 · Scale ×½ · Split [T])
Graphics     Point · Line · Curve [S][T] · Freehand · Picture [S][T] · Keep Tool [S][x]
             Polygon ⑂ [B][T] (Polygon | Lasso · Rectangle · Circle · Ellipse)
             Text ⑂ [B][T] (Text | Dynamic Text tokens)
Clipboard    Paste ⑂ [B] (Paste | Paste Special) · Cut · Copy · Copy Path [S]
```

### Analyze

```
Geoprocessing Tools [B][P] · Model Builder [B][W] · History · Environments · SQL [S][P]
Automate ★   Tasks [B][P] · Task ▾ [S] (Record Task · New Task From History · Import Task… · Run ▸)
             New Notebook [B] · Open Notebook… [S][F] · Notebook Pane [S][P]
             Script [S][P] · AI Assistant [B][P]
Workflows    Workflows ▾ [B]  (the 12 non-imagery sections: Suitability … Data Interop)
Tools        Gallery ×12 (unchanged; one name per tool, see the rule book)
Imagery ★    Raster Functions ⑂ [B] (Raster Functions | Function Editor · Indices · Process · Pixel Editor)
             Classify ▾ [B] (Classification Wizard · Classification Tools · Deep Learning Tools)
             Imagery Workflows ▾ [B] (Georeference · Change Detection · Spectral Analysis · Trend Analysis
                                      · Ortho Mapping · Reality Mapping · Mensuration · Create Hosted Imagery)
```

### View

```
Map          Globe [B][x] · Projection ▾ [B] · Global · Local [S][x] · Graticule [S][x]
Navigation   World [S] · Navigator [S][x] · Camera [S][D] · Magnifier [S][x]
             Navigation Mode ▾ [S] (Planar · First-Person, from Home)
             Identify ▾ [S] (pop-up mode, from Home)
Effects      Effects ▾ [B] ★ (Illumination ▸ · Weather ▸ · Visual Effect ▸ · Camera Effect ▸ · Postprocessing ▸)
             Color Vision ▾ [S] · Scene ▾ [S] ★ (Wireframe · Speed Display · View Clipping ▸)
Windows      Layers · Browser · Data Sources [B][P] · Details [S][P]
             Panes ▾ [B] ★ (every pane with its open state: Tools, Symbology, Labeling, History, Bookmarks,
                           AI Assistant, Tasks, Editing, Data Design, Workflows, …)
Labeling     Lock · Pause · View Unplaced [S][x] · Convert [S]   (map-wide; the layer Label toggle stays on Style)
Link         Side by Side · Link Views · Link Cursors [S][x]
Time         Time Slider [B][x] · Animation ▾ [B] (Add Keyframe · Remove · Play · Export… [F])
             Profile [S][P] · Time Series [S][P] (desktop)
Workspace    Browser View [S][x] · Workflow Manager [S][P] · Pane Sets ▾ [S]
```

### Publish (no layout open)

```
Export       Export Map [B][F] · Print Map · Capture To Clipboard [S]
Report       New Report [B] · Import Report… [S][F] · Report Pane [S][P]
             Report Section ▾ · Report Data ▾ · Report Output ▾ ★ [S] (Export Report… · Save As Report File…
                                                                       · Save As Report Template…)
Presentation New Presentation [B] · Import Presentation… [S][F] · Presentation Pane [S][P]
             Page ▾ · Presentation Output ▾ ★ [S] (Export Presentation… · Save As Presentation File…)
Share        Project Package ⑂ [B][F] (Project | Map · Mobile Map · Results · Deep Learning Model)
             Web Map ⑂ [B][F] (Web Map | Web Layer · Web Style · Locator · Geodata Service · Web Tool
                               · Web Scene · Web Link Chart) · Share Jobs [S][P]
Save As      Save As ⑂ [B][F] (Project… | Layout File · Map File · Layer File · Style Items · Color Ramps
                               · Project Template · Task Item)
```

### Layout ★ (contextual tab: appears and is selected while a layout is open)

```
Page         Page Size ▾ [B] · Snap [x] · Rulers [x] · Clear Guides [S]
Map Frames   Map Frame ⑂ [B] (Rectangle | Circle · Ellipse · Polygon [T] · Lasso [T]) · Pan/Zoom Map [B][x]
             Grid [x] · Extent Indicator ▾ · Map Series ▾ [S] (+ Previous · Next · Refresh while a series is on)
Surrounds    North Arrow · Scale Bar · Legend [B] · Colorbar · Table Frame · Chart Frame · More ▾ [S]
Graphics     Text ⑂ [B] (Text | Dynamic Text tokens) · Picture [B][F]
             Shapes ⑂ [B] (Rectangle | Circle · Ellipse · Line ▸ · Arrow)
Output       Export [B][F] · Print [B] · Close Layout [B]
```

### Plugins, Settings, Help

```
PLUGINS   Plugin Manager [B][W] · Install From File… [S][F] · per-plugin groups (unchanged)
SETTINGS  Customize: Ribbon ▾ · Quick Access ▾
          Display ★: Dark Mode [x] · GPU Rendering [x] · Item Pop-ups [x] · Measurement Units ▾ (Metric · Imperial)
                     · Feature Cache ▾ (Fill · Empty · Cancel · Auto Cache)
          Language: Language ▾ · Worldview ▾
          Start-up: Start With ▾ · Projection ▾ · Extent ▾
HELP      Help: Learning Resources [B] · Command Search [B][W] · Shortcuts [S][W] · Diagnostic Monitor [S][P]
          · Check for Updates [S] (desktop)
```

## 3. Mirrors kept on purpose (rule 2)

| Control | Places | Why |
|---|---|---|
| Select, Clear | Home › Selection · Edit › Selection · Data › Selection (Clear) | Editing and table work start from a selection; ArcGIS Pro does the same |
| Save | Backstage · Edit › Manage Edits | Saving edits is the end of the editing workflow |
| Attribute Table | Home › Selection › Attributes · Data › Table | Home is the everyday entry; Data is the table's own tab |

All other duplicates from `ribbon-audit.md` §2 are removed.

## 4. Where every removed or moved control went

| Today | Proposed | Route that remains |
|---|---|---|
| Home › Project › Toolbox | removed | Analyze › Tools; View › Panes ▾ |
| Home › Project › Import Project | removed | Backstage › Open |
| Home › Project › Browser | removed | View › Windows › Browser |
| Home › Project › Add Data (small) | removed | Home › Add › Add Data ⑂ |
| Home › Project › Add Folder | Home › Add Data ⑂ menu | |
| Home › Project › Import (maps) | Backstage › Start › Import Maps… | |
| Home › Project › Templates ▾ | Backstage › Start › From Template…; Publish › Save As ⑂ › Project Template | |
| Home › Navigate › Planar / First-Person, Identify | View › Navigation | |
| Home › Inspect › Locate | removed | Home › Navigate › Go To XY (same dialog) |
| Home › Inspect › Zoom Box | Home › Navigate | |
| Analyze › Selection | removed | Home › Selection |
| Analyze › Portal (Feature/Raster Analysis) | removed | Tools gallery, Tools pane, Workflows ▾, Imagery |
| Analyze › Raster, Image Analysis, Ortho, Reality, Alignment, Classification, Imagery Sharing, Mensuration, Imagery Tools | Analyze › Imagery (3 controls) | |
| Analyze › Workflows ▾ imagery entries | Analyze › Imagery › Imagery Workflows ▾ | |
| Analyze › Tasks, AI, Notebook groups | Analyze › Automate | |
| Data › Layer Templates | Home › Add Data ⑂ › Layer Templates ▸ | |
| Data › Subtypes, Domains, Attribute Rules, Contingent Values | Data › Fields › Data Design | the pane itself |
| Data › Feature Cache | Settings › Display › Feature Cache ▾ | |
| Style › Effects (Transparency) | Style › Visibility | |
| Style › Faces, Light and Shadow, Extrusion | Style › 3D | |
| Style › Layer › Attribute Table | removed | Data › Table; Home › Attributes |
| Style › Label Placement, Label Class | Style › Labels | |
| Edit › Status, Settings, Templates, Topology ▾, Ground To Grid, Corrections, Features › Modify | Edit › Editing Pane; Validate Topology | the Editing pane |
| Edit › Editor Tools ▾ › Edit Vertices, Move, Rotate, Scale… | removed from the menu | Edit › Features › Edit Vertices; Editing Pane |
| Data › Offline › Sync ▾, Open ▾, Remove ▾ | Data › Offline › Offline Areas ▾ | |
| Publish › Export (layout open) › Save Layout File | removed | Publish › Save As ⑂ › Layout File |
| Edit › Graphics (9 tools), Graphics and Text | Edit › Graphics (Point, Line, Curve, Freehand, Picture, Polygon ⑂, Text ⑂) | |
| View › Display › Dark Mode, GPU Rendering | Settings › Display | |
| View › Display › Graticule, View › View | View › Map | |
| View › Effects (6 menus) | View › Effects ▾ + Color Vision ▾ | |
| View › Scene › Wireframe, Speed Display, View Clipping | View › Scene ▾ | |
| View › Scene › Raise/Lower Priority | Style › Layer › Draw Priority ▾ | |
| View › Windows › Tools, Symbology, History, AI, Bookmarks, Labeling, Attribute Table | View › Panes ▾ | each tab's own button |
| View › Labeling › Label | removed | Style › Labels › Label |
| View › Workspace › Tasks | removed | Analyze › Automate › Tasks |
| View › Workspace › Item Pop-ups | Settings › Display | |
| View › Animation, Profile | View › Time | |
| Publish › Output › Export Features | removed | Data › Export › Export Features |
| Publish › Output › Export To 3D | Data › Export | |
| Data › Export ▾ › Export Features (tool) | removed | Analyze › Tools gallery › Export Features |
| Home › Inspect › Measure ▾ › Metric / Imperial | Settings › Display › Measurement Units ▾ | |
| Publish › Output › Report ▾, Presentation ▾ | Publish › Report Output ▾, Presentation Output ▾ | |
| Publish › Layout (Export/Print Layout) | Layout › Output | |
| Publish › Share As › Deep Learning | Publish › Project Package ⑂ menu | |
| Publish › Thumbnail | Backstage › Current Project | |
| Publish › layout groups | Layout tab | |
| Plugins › Browse Plugins | removed | Plugin Manager (Browse tab) |
| Help › About | removed | Backstage › About |
| Help › Performance | Help › Help | |

Every `ui.*` control that leaves the ribbon keeps a route listed above, so rule 1 needs no new
`UI_COMMANDS` entries for this layout. The new Panes ▾ menu is the route for pane toggles.

## 5. What it takes to build

- **No new widget types:** split buttons use `big_split_menu`; Panes ▾, Effects ▾ and Scene ▾ use
  `big_menu` / `small_menu` with checkboxes, as Visual Effect ▾ does today.
- **New behaviour:**
  - the contextual Layout tab: the tab strip shows it only while `session.active_layout` is set
    and switches to it when a layout opens (today `layout.new` switches to Publish);
  - three new backstage entries (From Template, Import Maps, thumbnails);
  - a Settings › Display group.
- **Renames that tests and catalogs see:** Home › "Import Project" goes, and Data Design and
  Editing Pane are new labels. The `tl!` catalogs pick up new strings as usual (AGENTS.md).
  Saved Quick Access and hidden-tab settings key on command ids and tab names; only the new
  Layout tab name is added.
- **Command Search locations** (`chrome.rs` `UI_COMMANDS`) need updating for every moved `ui.*`
  command (rule 13).
- **Suggested order** (independent pieces):
  1. remove duplicates and merge pane buttons;
  2. split buttons;
  3. Analyze Imagery and Automate;
  4. View and Settings moves;
  5. Layout tab;
  6. backstage additions.

## 6. Open questions for the community

1. **Fixed default or palette** for the map-tool split buttons (Select shapes, graphics Polygon)?
   #831 (implemented by #855) fixes the default; ArcGIS Pro and stock QGIS switch to the last-used tool
   (`ribbon-rulebook.md` §4). And is 3 alternatives the right minimum for a split button
   (rule 5)?
2. **Contextual Layout tab, or keep layout groups at the front of Publish?** The tab follows
   ArcGIS Pro; staying in Publish needs no tab-strip change.
3. **Web Data:** keep the 33-entry dropdown, or open the Data Sources pane's public-source
   library with search (rule 6)?
4. **Imagery:** an Analyze group (proposed), or a separate Imagery tab hidden by default for
   users who don't work with rasters?
5. **Usage tiers:** the ●●● / ●● / ● ratings behind these choices are estimates. Which ones are
   wrong for your work?
