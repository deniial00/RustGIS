# Ribbon audit: menu items, usage and entry points

The facts only. Proposals (split buttons, rules, removals) are in `ribbon-rulebook.md`;
how ArcGIS Pro and QGIS do it is in `ribbon-arcgis.md` and `ribbon-qgis.md`.

Basis for a community discussion on which features need a place in the ribbon.

- Source: `crates/ui-egui/src/ribbon.rs` and `backstage.rs`. Destinations were traced through the
  dispatch branches in `lib.rs` (e.g. `ui.searchOpenData` opens the Search Open Data dialog;
  `ui.stac*` opens the `stac` dialog; the Web Data entries that call `ui.dialog` open the
  **Web Data pane**, not a dialog).
- **The usage tiers are estimates.** The repo has no usage telemetry. The tiers come from how
  central each feature is to common desktop GIS work, so they are a starting point for votes,
  not measurements. A tier rates a ribbon control (button, toggle, dropdown) or a backstage
  entry: how often people reach for that control at all. Entries inside a dropdown are rated
  only where they launch a different feature (e.g. the Start page cards). Values of one setting
  (a page size, a projection, a blend mode) are not rated; the dropdown's tier covers them. That
  is why Appendix A has no tiers.
- Pane contents (e.g. the sections inside the Workflows pane) are not counted as menu items.
  They are listed only where a ribbon button opens a pane at a specific section.
- Section 1 shows every tab, group and control. Long or data-driven dropdowns are summarised
  there by count; **Appendix A lists every entry in them**.
- `big_split_menu` (`widgets.rs`) has one ribbon user: Home › Selection › Select, added by PR #855
  (merged on main, closes #831). The rest of this audit was read before that merge; #855 changes
  only the Select control (below). ROADMAP.md lists
  no ribbon reorganisation as underway; it does plan an on-map right-click menu, which is another
  entry point (see rule 1 in `ribbon-rulebook.md`).

## Summary

- About **290 controls sit directly on the ribbon** (counted by hand, ±5). That excludes dropdown
  entries, plugin buttons, and the ~30 extra controls shown while a layout is open.
- **27 commands or panes can be reached from two or more places in the ribbon** (section 2).
- The Analyze tab has **8 imagery groups with 18 buttons**. All of them open the Workflows pane at
  one of 15 sections, and 13 of those sections are also in the Analyze › Workflows dropdown.
- **8 Edit-tab controls open the Editing pane**, and **5 Data-tab controls open the Data Design
  pane**.

## 1. Inventory

```
FREQ   ●●● core (most sessions)   ●● regular (specific workflows)   ● niche / specialist
OPENS  [P] side pane  [D] dialog  [W] own window  [F] file picker  [T] map tool (mode)
       [x] toggle/radio  ▾ dropdown  ▸ submenu   (blank = runs immediately)
FLAGS  =dup duplicate elsewhere
```

### Project button (backstage)

```
Navigation          Back to the map
                    Start              → Start page (below)
                    New                → Start page (below)
                    Open               [F]                          =dup Home›Import Project
                    Save               (closes the backstage)       =dup Edit›Save
                    Save As            [F]                          =dup Publish›Save As›Project
                    About              → About page                 =dup Help›About
Start page          New Project:  Map (blank, with basemap) ●● · World Atlas (sample) ●
                                  Open… [F] ●●● · QGIS Project… [F] ●
                    Current Project:  Name · Location · Maps · Layers   (information only)
About page          Version, description, project and licence links   (information only)
```

### Home

```
Project
  New Map                         ●●
  Toolbox                         ●●   [P] Tools          =dup ×3
  Import Project                  ●●   [F]                =dup Backstage›Open
  New Layout ▾                    ●●
    page sizes · Publication Layout ▸ (11 profiles) · Blank Letter page · Import Layout… [F]
  Browser                         ●    [P] Browser        =dup View›Windows
  Add Data                        ●●●  [F]                =dup Add›Add Data
  Add Folder                      ●    [F]
  Import (maps)                   ●    [F]
  Templates ▾                     ●    New Custom… [F] · Open Custom… [F]
Navigate
  Pan                             ●●●  [T]
  Bookmarks ▾                     ●●   list · New Bookmark… [D] · Manage [P] · Import… [F] · Export… [F]
  Full Extent                     ●●●
  Zoom In · Zoom Out              ●●
  Previous · Next                 ●●
  Go To XY                        ●    [D]                =dup Inspect›Locate
  Planar / First-Person ▾         ●    [x]
  Identify ▾                      ●    [x] pop-up mode
  Find Place                      ●●   [D]
  Directions                      ●    [P]
Add
  Basemap ▾                       ●●●  gallery: built-in · web · planets · tile layers
  Add Data ▾                      ●●●
    Data… [F] · Delimited Text… [F] · Geocode… [D] · From URL… [D] · From Service… [D] · Zarr… [D]
    Database… [D] (desktop) · S3… [D] · Connections… [P] · New Connection ▸ [D] · Graphics Layer
    Sample data ×4
  Web Data ▾                      ●●   33 entries (22 + France ▸ 1 + United States ▸ 10)
    13 → Web Data [P] · 14 → Search Open Data [D] · 6 → STAC [D]
Selection
  Select (split button)           ●●●  [T]  Select (click/box) | Polygon [T] · Lasso [T] · Radius [T]  (#855)
  Select By Attributes            ●●   [D]                =dup Analyze›Selection
  Select By Location              ●●   [P] Tools          =dup Analyze›Selection
  Clear                           ●●●                     =dup Data›Selection
  Zoom To                         ●●
  Attributes                      ●●●  table              =dup ×4
Inspect
  Measure ▾                       ●●   Distance [T] · Area [T] · units [x]
  Locate                          ●    [D] Go To XY       =dup
  Zoom Box                        ●    [T]
  Infographics                    ●    [P]
```

### Data (feature layer; a raster layer swaps the middle groups)

```
Layer Templates   Layer Templates ▾ (7)                        ●
Definition Query  Definition Query                     [D]     ●●
Table             Attribute Table                              ●●●  =dup
Selection         All · Switch · Clear                         ●●   Clear =dup
                  Layer From Selection                         ●●
Data Design       Fields [D] · Calculate [D]                   ●●
                  Subtypes · Domains · Attribute Rules · Contingent Values   ●  all [P] Data Design
Archiving         Archive                              [x]     ●
Relationship      Related Data ▾ (list · New Relationship… [P] Data Design)  ●
Export            Export ▾ (Export Features… [D] · Export Features tool [P]) ●●  =dup Publish
Offline           Download Map · Sync ▾ · Open ▾ · Remove ▾    ●
Feature Cache     Fill · Empty · Cancel · Auto Cache [x]       ●
raster:           Information › Raster Information [P] Details ● · Export › Export Raster [F] ●●
```

### Style (selected layer)

```
Visibility Range  Out Beyond ▾ · In Beyond ▾ · Clear Limits    ●●
Effects           Transparency slider                          ●●
Drawing           Symbology [P] ●●● · Primary Symbology ▾ ●●● · Color ●●● · Outline ●● · Size / No Fill ●●
Drawing Effects   Masking ▾ · Display Filters ▾ · Aggregation ▾ (points)        ●
Extrusion         Type ▾ · Factor (polygons)                   ●
Faces             Face Culling [x] · Lighting [x]              ●   (shown when extruded)
Light and Shadow  Enable Lighting [x] · Cast Shadow [x]        ●   "Lighting" and "Enable Lighting" set the same field
Layer             Zoom To Layer · Attribute Table =dup · Visible [x] · Remove   ●●
Labels            Label [x] =dup View›Labeling ●●● · Properties [P] Labeling ●●
Label Range       Out/In Beyond ▾ · Closest ▾ · Farthest ▾ · Clear Limits       ●
Label Placement   Placement ▾                                  ●●
Label Class       Field                                        ●●●
Text Symbol       Size · Color · Bold [x] · Halo [x]           ●●
Styles            Styles ▾ (saved · New From Layer · Add Style File… [F] · Import Styles… [F] · Add Ramp File… [F])  ●
raster:           Compare › Swipe [x] ●● · Raster Effects (Blend ▾, Masking ▾, Rotation) ●
                  Rendering (Symbology ▾ ●●● · Stretch ▾ ●● · DRA [x] ●● · Resampling ▾ ● · Bands ▾ ●●)
                  Enhancement (Brightness · Contrast · Gamma · Reset) ●● · Layer
no layer:         a hint ("Select a layer in Layers to style it.") · Styles
```

### Edit

```
Manage Edits      Edit [x] ●●● · Save ●●● (=dup) · Undo · Redo ●●●
                  Status · Settings · Templates · Topology ▾   ●   all [P] Editing
Snapping          Snapping [x]                                 ●●
Features          Create [T] ●●● · Delete ●●● · Edit Vertices [T] ●● · Modify [P] Editing ●●
Tools             Editor Tools ▾ ●● Merge · Explode · Flip · Rotate 90° · Scale ×2 · Scale ×½ · Split [T]
                                    · Edit Vertices [T] =dup · Move, Rotate, Scale… [P] Editing
Elevation         None / Constant… [P] / Surface ▾             ●
Corrections       Ground To Grid [P] Editing                   ●
Graphics          Point · Line · Curve · Freehand · Polygon · Lasso · Rectangle · Circle · Ellipse [T] ●●
                  Keep Tool [x] ●
Graphics and Text Text [T] ●● · Dynamic Text ▾ [T] ● · Picture [T] ●
Clipboard         Paste ▾ (Paste · Paste Special) ●●● · Cut · Copy ●●● · Copy Path ●
```

### Analyze

```
Geoprocessing     History [P] ●● =dup · Tools [P] ●●● =dup ×3 · Model Builder [W] ●●
                  Script [P] ● · SQL [P] ●● · Environments [P] Workflows ●
Tasks             Tasks [P] ● =dup View · Task ▾ (Record · From History · Import… [F] · Run …) ●
AI                AI Assistant [P] ●● =dup View
Portal            Feature Analysis ▾ · Raster Analysis ▾ → Tools / Workflows panes   ●
Workflows         Workflows ▾ → [P] Workflows at a section ●● (13 of its entries repeat the imagery groups)
Raster            Raster Functions ●● · Function Editor ●   [P] Workflows  =dup Image Analysis
Tools             Gallery ×12 → [P] Tools ●●● (Buffer, Clip, Erase, Dissolve, Summarize Within, …, IDW)
Selection         By Attributes [D] · By Location [P]          =dup Home
Notebook          New Notebook [P] · Add and Open [F] · Notebook Pane [P]   ●
┄ imagery: 8 groups, 18 buttons, all [P] Workflows at a section ┄
Ortho Mapping     New Workspace · Options      ●  (both open the same section)
Reality Mapping   New Workspace · Options      ●  (both open the same section)
Alignment         Georeference                 ●
Image Analysis    Raster Functions =dup · Function Editor =dup · Change Detection · Spectral Analysis  ●
Image Classif.    Classification Wizard · Classification Tools · Deep Learning Tools   ●
Imagery Sharing   Create Hosted Imagery        ●
Mensuration       Mensuration · Results        ●  (both open the same section)
Imagery Tools     Process · Indices · Pixel Editor   ●
```

### View

```
Display           Graticule [x] ●● · Dark Mode [x] ● · GPU Rendering [x] ●
View              Globe [x] ●● · Projection ▾ (presets · Coordinate System… [D]) ●● · Global · Local [x] ●
Effects           Illumination ▾ · Weather ▾ · Visual Effect ▾ · Camera Effect ▾ · Postprocessing ▾ · Color Vision ▾  ●
Scene             Wireframe [x] · Raise/Lower Priority · View Clipping ▾ · Speed Display [x]   ●
Windows           Layers ●●● · Browser =dup · Data Sources ●● · Tools =dup · Symbology =dup · History =dup
                  Details ● · AI Assistant =dup · Bookmarks =dup · Labeling =dup · Attribute Table =dup  (all [P])
Labeling          Label [x] =dup · Lock [x] · Pause [x] · View Unplaced [x] · Convert   ●
Navigation        World ●● · Navigator [x] ● · Camera [D] ● · Magnifier [x] ●
Link              Side by Side [x] ●● · Link Views [x] ● · Link Cursors [x] ●
Workspace         Browser View [x] · Tasks [P] =dup · Workflow Manager [P] · Pane Sets ▾ · Item Pop-ups [x]  ●
Animation         Add (n) · Remove · Play [x] · Export [F]     ●
Time              Time Slider [x]                              ●●
Profile           Profile [P] ● · Time Series [P] ● (desktop)
```

### Publish

```
┄ only while a layout is open ┄
Page              Page Size ▾ ●● · Snap [x] · Rulers [x] · Clear Guides (when guides exist)   ●
Map Frames        Map Frame ▾ (Rect · Circle · Ellipse · Polygon [T] · Lasso [T]) ●● · Pan/Zoom Map [x] ●●
                  Grid [x] · Extent Indicator ▾ · Map Series ▾ (+ Prev/Next/Refresh while a series is on)   ●
Map Surrounds     North Arrow · Scale Bar · Legend ●●● · Colorbar ●● · Table Frame · Chart Frame · More ▾  ●
Shapes            Rectangle · Circle · Ellipse · Line ▾ · Arrow  ●
Text              Text ●●● · Dynamic Text ▾ ●● · Picture [F] ●●
Export            Export [F] ●●● · Print ●● · Save Layout File [F] ● · Show Map ●●   =dup Layout/Save As
┄ always ┄
Report            New Report [P] · Report Section ▾ · Data ▾ · Report Pane [P] · Import Report [F]   ●
Presentation      New Presentation [P] · Page ▾ · Import [F] · Presentation Pane [P]   ●
Output            Export Map [F] ●●● · Export Features [D] ●● =dup · Print Map ●● · Capture To Clipboard ●●
                  Report ▾ · Presentation ▾ · Export To 3D [F]   ●
Package           Project Package [F] ●● · Map · Mobile Map · Results [F]   ●
Share As          Web Map [F] ●● · Web Layer · Web Style · Locator · Geodata Service · Web Tool · Web Scene
                  · Web Link Chart · Deep Learning (saves a dlPackage) [F] ● · Jobs [P] ●
Layout            Export Layout [F] · Print Layout             =dup Export (layout groups)
Save As           Project [F] ●● =dup Backstage · Layout File · Map File · Layer File · Style Items
                  · Color Ramps · Project Template · Task Item [F]   ●
Thumbnail         Create · Import [F] · Map Thumbnail · Import [F]   ●
```

### Plugins, Settings, Help

```
PLUGINS   Plugin Manager [W] ●● · Install From File [F] ● · Browse Plugins [W] ● · one group per enabled plugin
          (panel [P] · commands · tools [P] Tools); plugins with their own tab get that tab instead
SETTINGS  Customize: Ribbon ▾ · Quick Access ▾ │ Language ▾ · Worldview ▾ │ Start With ▾ · Projection ▾ · Extent ▾  (all ●)
HELP      Diagnostic Monitor [P] ● │ Learning Resources (browser) ●● │ About [W] =dup Backstage
          · Check for Updates (desktop) ● · Command Search [W] ●● · Shortcuts [W] ●
```

### Approximate size per tab

| Tab | Controls on the ribbon | Rated ● (niche) | Duplicates |
|---|---|---|---|
| Home | 34 | 12 | 9 |
| Data | 24 | 15 | 3 |
| Style (polygon layer) | 36 | ~16 | 3 |
| Edit | 33 | 12 | 3 |
| Analyze | 49 | 33 | 10 |
| View | 53 | 37 | 10 |
| Publish (+~30 with a layout open) | 44 | 30 | 6 |
| Plugins / Settings / Help | 3 / 7 / 6 | — | 1 |

## 2. Where the same thing appears more than once

| Command or pane | Places | Locations |
|---|---|---|
| Editing pane | 8 | Edit: Status · Settings · Templates · Topology Options · Modify · Ground To Grid · Editor Tools › Move… · Elevation › Constant… |
| Data Design pane | 5 | Data: Subtypes · Domains · Attribute Rules · Contingent Values · Related Data › New Relationship… |
| Attribute table | 4 | Home › Selection · Data › Table · Style › Layer · View › Windows |
| Symbology pane | 4 | Style › Drawing · Style › Layer (raster) · raster Symbology ▾ · View › Windows |
| Tools pane | 3 | Home › Project › Toolbox · Analyze › Geoprocessing · View › Windows |
| Export Features | 3 | Publish › Output · Data › Export ▾ · Analyze › Tools gallery |
| 13 imagery workflow sections | 2 each | Analyze › Workflows ▾ and the imagery groups |
| Raster Functions, Function Editor | 2 each | Analyze › Raster · Analyze › Image Analysis |
| Ortho, Reality, Mensuration | 2 buttons → 1 section | New Workspace = Options; Mensuration = Results |
| Select By Attributes, Select By Location | 2 each | Home › Selection · Analyze › Selection |
| Go To XY dialog | 2 | Home › Navigate › Go To XY · Home › Inspect › Locate |
| Browser, History, Tasks, AI Assistant, Bookmarks, Labeling panes | 2 each | their own tab · View › Windows (Bookmarks: Home › Bookmarks ▾ › Manage) |
| Label on/off | 2 | Style › Labels · View › Labeling |
| Clear selection | 2 | Home › Selection · Data › Selection |
| Add Data…, Open project, Save As, About | 2 each | Home, Publish or Help · Backstage |
| Export / Print / Save Layout File | 2 each | Publish › Layout or Save As · Publish › Export (with a layout open) |
| Extrusion lighting | 2 | Style › Faces › Lighting and Light and Shadow › Enable Lighting: the same field under two names and two commands |
| Edit Vertices | 2 | Edit › Features · Editor Tools ▾ |

Some of these mirrors may be deliberate (for example a contextual tab repeating a general
command). Rule 2 in `ribbon-rulebook.md` proposes which ones stay.

### Smaller problems found along the way

- Command Search shows the SQL Pane under "Analyze › Workflows"; it is actually in
  Analyze › Geoprocessing (`chrome.rs:641`). MCP Relay is listed under "Help › Help" but has no
  ribbon button. These location strings are maintained by hand, so they drift.
- Share As › Deep Learning saves a package (`ui.saveItem` kind `dlPackage`, "Package a trained
  deep learning model"); the other Package-type saves are in the Package group.
- Home › "Import Project" runs `ui.open`, the same command as Backstage › Open.
- The same tool has two names: the Analyze › Tools gallery says "Bounding Geometry" and
  "Summarize Within", while Analyze › Portal › Feature Analysis lists "Convex Hull" and
  "Count Points" (`convexHull`, `countPoints`).
- The two Dynamic Text menus offer different tokens. Edit › Graphics and Text has 7
  (`graphics::TOKENS`, including `{layers}` and `{center}`); Layout › Text has 8 (an inline list
  with `{page} / {pages}`, `{pageName}` and `{layout}`).

## Appendix A. Every entry of the long and data-driven dropdowns

Labels as in source (English). Entries that depend on the project (bookmarks, layers, fields,
tasks, reports, styles, offline areas, relationships, pane sets) are listed by what they show.

```
HOME
Project › New Layout ▾
  Letter (216 × 279 mm) · Legal · Tabloid · A5 · A4 · A3 · A2 · A1 · A0
  Publication Layout ▸
    A4 portrait sheet · A4 landscape sheet · A3 portrait sheet · A3 landscape sheet
    A2 portrait sheet · A2 landscape sheet
    Journal figure, 1 column (90 mm) · 1.5 columns (140 mm) · 2 columns (190 mm)
    Thesis figure, full page (160 × 225 mm) · half page (160 × 110 mm)
  ─ Blank Letter page · Import Layout… [F]
Project › Templates ▾          New Custom Template… [F] · Open Custom Template… [F]
Navigate › Bookmarks ▾          <each bookmark: go · remove> ─ New Bookmark… [D] · Manage Bookmarks [P]
                                ─ Import Bookmarks… [F] · Export Bookmarks… [F]
Navigate › Planar ▾             Planar Navigation · First-Person Navigation
Navigate › Identify ▾           Topmost Layer · Visible Layers · Selectable Layers · Selected in Layers · No Pop-ups
Add › Basemap ▾
  Basemaps:          Paper · Midnight · Ocean · Atlas · Blank
  Web Basemaps:      [x] Stack on the current basemaps
                     OpenFreeMap Liberty · Bright · Positron · Dark · Fiord · USGS Imagery (Earth basemap)
                     link: Open the Basemaps tab / Turn on / Install the Basemaps plugin [W]
  Celestial Bodies:  Earth · Moon · Mars · Callisto · Charon · Europa · Ganymede · Io · Mercury · Pluto · Titan · Venus
    (off Earth, the body's basemaps)
      Moon:     Hillshaded Albedo · OpenPlanetaryMap Basemap
      Mars:     Viking MDIM2.1 · OpenPlanetaryMap Basemap · MOLA Colour Elevation
                · MOLA Shaded Colour Elevation · MOLA Shaded Grey Elevation · Hillshade
      Callisto, Europa, Ganymede: Galileo / Voyager
      Charon:   New Horizons Mosaic
      Io:       Galileo SSI Colour
      Mercury:  MESSENGER Colour Mosaic · MESSENGER Basemap
      Pluto:    New Horizons Mosaic · New Horizons Colour Shaded Relief
      Titan:    Cassini ISS Mosaic · Cassini HiSAR Mosaic
      Venus:    Magellan C3-MDIR Colour
  Web Tile Layers:   <each tile layer: shown [x] · opacity · ▲ · ▼ · remove>
                     Add Tile Layer… [D] · Reload · Remove All
Add › Add Data ▾
  Data… [F] · Delimited Text… [F] · Geocode Addresses… [D] · From URL… [D] · From Service… [D]
  Zarr… [D] · Database… [D] (desktop) · S3… [D] · Connections… [P Data Sources]
  New Connection ▸ [D]: PostgreSQL/PostGIS* · MySQL/MariaDB* · SQL Server* · Web service (detected)
                        · WMS · WMTS · WFS · WCS · OGC API · ArcGIS REST · Catalogue (CSW) · STAC API · S3
                        (* desktop only)
  Graphics Layer
  ─ Sample data (Natural Earth): World Countries · Populated Places · Major Rivers · Major Lakes
Add › Web Data ▾   ([P] = Web Data pane · [S] = Search Open Data dialog · [STAC] = STAC dialog)
  ArcGIS Portal… [S] · Boundaries… [P] · Dynamical… [S] · Earthdata GIS… [S]
  Fields of the World… [P] · GeoLens… [S] · Hugging Face… [S] · NASA Earthaccess… [P]
  NASA Earthdata… [P] · Natural Earth… [P] · Ocean Data Platform… [S] · OpenAerialMap… [S]
  OpenStreetMap… [P] · Overture Maps… [P] · Planet Open Data… [STAC] · Portolan… [STAC]
  Satellite Embeddings… [P] · Search Open Data… [S] · Sentinel-2 Explorer… [STAC]
  Source Cooperative… [P] · STAC Search… [STAC] · Vantor… [STAC]
  ─ France ▸        IGN LiDAR HD… [S]
    United States ▸ EnviroAtlas… [P] · FEMA NFHL… [P] · Tennessee GIS… [S] · The National Map… [P]
                    · US Federal GIS… [S] · US Local GIS… [S] · US State GIS… [S] · USGS 3DEP DEM… [S]
                    · USGS LiDAR… [STAC] · USGS NLDI… [P]
Inspect › Measure ▾            Measure Distance [T] · Measure Area [T] ─ Metric · Imperial

DATA
Layer Templates ▾   Points · Lines · Polygons · Places (name, category, address, rating)
                    · Routes (name, surface, length) · Areas (name, land use, area)
                    · Observations (observer, date, species, count)
Related Data ▾      <each relationship> ─ New Relationship… [P]
Export ▾            Export Features… [D] · Export Features (tool) [P]
Offline › Sync ▾ / Open ▾ / Remove ▾   <each offline area>

STYLE
Visibility Range › Out Beyond / In Beyond   <None> · 1:1,000 · 1:10,000 · 1:50,000 · 1:100,000 · 1:500,000
                                            · 1:1,000,000 · 1:5,000,000 · 1:10,000,000 · 1:50,000,000 · 1:100,000,000
Primary Symbology ▾   Single Symbol · Unique Values ▸ <fields> · Graduated Colors ▸ <numeric fields>
                      · Proportional Symbols ▸ <numeric fields> (points)
Masking ▾             No mask · <each other polygon layer>
Display Filters ▾     expression box · Set Filter · Clear Filters
Aggregation ▾         None · Hexagons · Squares   (points)
Extrusion › Type ▾    None · Constant height · By <numeric field>
Label Range › Out/In Beyond   same scale list as Visibility Range
Label Range › Closest ▾ / Farthest ▾   <None> · 1 km · 10 km · 100 km · 1,000 km · 10,000 km
Label Placement ▾     Automatic · Centered · Above · Below · Left · Right · Above Right · Above Left
                      · Below Right · Below Left · Parallel · Curved
Label Class › Field   <fields>
Styles ▾              <each saved style> ─ New Style From Layer · Add Style File… [F]
                      · Import Styles From… [F] · Add Color Ramp File… [F]
raster › Layer Blend ▾       Normal · Multiply · Screen · Overlay · Darken · Lighten · Difference
raster › Symbology ▾         Stretch · RGB · Classify · Unique Values · Colormap · Shaded Relief
                             ─ Symbology Pane… [P]
raster › Stretch Type ▾      None · Minimum Maximum · Custom Min/Max · Percent Clip · Standard Deviation
                             · Histogram Equalize
raster › Resampling ▾        Nearest Neighbor · Bilinear
raster › Band Combination ▾  Natural Color (1, 2, 3) · Color Infrared (4, 1, 2) ─ Red ▸ · Green ▸ · Blue ▸ <bands>

EDIT
Topology ▾          Topology Options… [P] · Error Inspector (validate) [P]
Editor Tools ▾      Merge · Explode · Flip Line · Rotate 90° · Scale ×2 · Scale ×½ · Split (click the line) [T]
                    · Edit Vertices [T] · Move, Rotate, Scale… [P]
Elevation ▾         None · Constant… [P] · Surface: <each raster layer>
Dynamic Text ▾      Map name {map} · Map scale {scale} · Current date {date} · Current time {time}
                    · Coordinate system {crs} · Number of layers {layers} · Map centre {center}
Paste ▾             Paste · Paste Special (as a new layer)

ANALYZE
Task ▾              Record Task / Stop Recording · New Task From History · Import Task… [F] ─ Run <each task>
Portal › Feature Analysis ▾   Buffer · Clip · Erase · Dissolve · Convex Hull · Feature To Point · Simplify
                              · Count Points · Calculate Geometry   (each → Tools pane)
Portal › Raster Analysis ▾    Raster Functions · Suitability Modeler · Viewshed · Interpolate (IDW)
                              → Workflows pane; then up to 40 raster tools → Tools pane
Workflows ▾  (each → Workflows pane at that section)
  Suitability Modeler · Zonal Statistics · Visibility Analysis · Exploratory 3D Analysis
  · Geostatistical Wizard · Network Analysis · Neighborhood Explorer · Simulation · Data Engineering
  · Exploratory Analysis · Embeddings · Data Interop · Georeference · Change Detection · Trend Analysis
  · Spectral Analysis · Classification Wizard · Classification Tools · Deep Learning Tools
  · Hosted Imagery · Indices · Process · Pixel Editor · Mensuration · Ortho Mapping · Reality Mapping
Tools gallery (12 big buttons, each → Tools pane)
  Buffer · Clip · Erase · Dissolve · Summarize Within · Bounding Geometry · Feature To Point
  · Simplify · Calculate Geometry · Summary Statistics · Export Features · IDW Interpolation

VIEW
Projection ▾        Web Mercator · Equirectangular (Plate Carrée) · Miller Cylindrical
                    · Mollweide (equal-area world) · Lambert Cylindrical Equal Area · Robinson · Equal Earth
                    · Natural Earth · Winkel Tripel · Eckert IV (equal-area world) · North Polar Stereographic
                    · South Polar Stereographic · Europe Equal Area (ETRS89 LAEA)
                    · USA Contiguous Albers Equal Area · Albers Equal Area Conic · Lambert Conformal Conic
                    · VN-2000 / TM-3 (Vietnam) · VN-2000 / UTM zone 48N · VN-2000 / UTM zone 49N
                    ─ Coordinate System… [D]
Illumination ▾      Off · Now · 06:00 UTC · 12:00 UTC · 18:00 UTC · 00:00 UTC
Weather ▾           None · Haze · Fog · Rain · Snow
Visual Effect ▾     [x] Vignette · [x] Edges · [x] Stars
Camera Effect ▾     [x] Tilt-shift · [x] Film grain
Postprocessing ▾    Brightness · Contrast · Saturation · Sepia (sliders) · Reset Effects
Color Vision ▾      Normal vision · Protanopia (no red) · Deuteranopia (no green) · Tritanopia (no blue)
                    · Achromatopsia (no colour)
View Clipping ▾     None · 100 km · 500 km · 1,000 km · 3,000 km
Pane Sets ▾         <each saved set> ─ Save as Mapping · Save as Editing · Save as Analysis

PUBLISH
Page Size ▾         Letter · Legal · Tabloid · A5 · A4 · A3 · A2 · A1 · A0 ─ Portrait · Landscape
Map Frame ▾         Rectangle · Circle · Ellipse · Polygon (draw) [T] · Lasso (draw) [T]
Extent Indicator ▾  <each other map frame>
Map Series ▾        <each feature layer of the frame's map> · Turn Off
More ▾ (surrounds)  Neatline · Backdrop
Line ▾              Horizontal · Vertical · Diagonal ↘ · Diagonal ↗
Dynamic Text ▾      Current date {date} · Current time {time} · Map scale {scale} · Map name {map}
                    · Coordinate system {crs} · Page number {page} / {pages}
                    · Map series page name {pageName} · Layout name {layout}
Report Section ▾    Supplemental Page · Supplemental Page After · New Template (a layout page)
                    · Existing Template ▸ <layouts> · Layout File… [F]
                    · Related Report ▸ <other reports> · Subreport ▸ <relationships>
Report › Data ▾     Field ▸ · Autofill Fields · Statistics ▸ <field> ▸ count · sum · mean · min · max
                    · Group ▸ · Attachments ▸ · Dynamic Picture ▸ · Chart Frame ▸   (▸ = fields)
Presentation › Page ▾   Map (the current view) · Blank · Image… [F] · Video… [F]
Output › Report ▾       Export Report… [F] · Save As Report File… [F] · Save As Report Template… [F]
Output › Presentation ▾ Export Presentation… [F] · Save As Presentation File… [F]

SETTINGS
Ribbon ▾            [x] Home · Data · Style · Edit · Analyze · View · Publish · Plugins · <plugin tabs>
Quick Access ▾      [x] Save As · Export Map · Print Map · Attribute Table · Full Extent · Clear Selection
                    · Tools Pane · Command Search
Language ▾          Automatic (system language) · English · Deutsch · Español · Français · Italiano
                    · Nederlands · Português · Русский · Türkçe · Azərbaycanca · Bahasa Indonesia
                    · Tiếng Việt · فارسی · 简体中文 · 日本語 · 한국어 · ქართული · বাংলা
Worldview ▾         Automatic (system time zone and region) · Default · China
Start With ▾        World Atlas · Blank Map · Last Project (desktop) · Choose Project… [F] (desktop)
Start-up Projection ▾  Web Mercator · Globe · <the projection list above> · EPSG or ESRI code + Use
Start-up Extent ▾   Full Extent · Use Current View
```
