# Ribbon rule book (proposal)

A proposal for discussion: which features get a ribbon control, and which kind. It builds on the
inventory in `ribbon-audit.md` (tabs, groups, controls, usage tiers ●●●/●●/●, duplicates) and on
how ArcGIS Pro (`ribbon-arcgis.md`) and QGIS (`ribbon-qgis.md`) handle the same questions.

## 1. Rules

1. **The ribbon is for finding things, not the only way in.** Taking a button off the ribbon
   never removes the feature, but its other routes have to be checked first. Engine commands
   also run from the CLI. UI-only `ui.*` commands (panes, dialogs, file pickers) do not, and
   Command Search lists only those in `UI_COMMANDS` (`chrome.rs`). So before a `ui.*` button
   goes, its command must be added to `UI_COMMANDS`, or reachable from a pane or right-click
   menu, and the proposal names that route.
2. **One home per command, with deliberate mirrors only.** A command, or a pane with the same
   parameters, gets one ribbon button unless a workflow tab needs it at hand. ArcGIS Pro and the
   QGIS ribbon prototype both repeat Selection on two tabs, and Pro repeats the labeling Pause,
   Lock and View Unplaced controls (`ribbon-arcgis.md` §4, `ribbon-qgis.md` §5), so a blanket ban
   would be stricter than either. Allowed mirrors:
   - Home may repeat at most ~8 core items from other tabs.
   - A workflow tab may repeat a group it can't work without (Selection on Edit), keeping the
     same command, label and icon.
   - Layer-context groups may repeat what the Layers right-click menu offers.
   - The backstage owns Open, Save, Save As and About; tabs don't repeat them.

   Not allowed: two buttons on the same tab for the same thing (Go To XY and Locate), two names
   for one setting (Lighting and Enable Lighting), and several buttons that open one pane
   (rule 3).
3. **One button per pane.** Several buttons that open the same pane become one button. If the
   pane has sections, use a dropdown of sections.
4. **Size follows how often it's used.**
   - ●●● → big button.
   - ●● → small button, or an entry in a split or dropdown.
   - ● → dropdown entry, the pane itself, or Command Search only.
   - A ● item at top level needs a stated reason, for example being the only way to discover a
     feature.
5. **Split button only when there is a real default:**
   - The main action covers most uses.
   - The menu entries are variants of the same action with the same kind of result.
   - The default is fixed and never becomes "last used" (#831, implemented by #855). This is a deliberate choice
     that differs from ArcGIS Pro, whose split buttons and tool palettes (Select is a palette)
     promote the last picked entry. Stock QGIS also uses last-used split buttons for selection
     and measure. The QGIS ribbon prototype and QGIS's Create Annotation Layer keep a fixed
     default (`ribbon-arcgis.md` §2, `ribbon-qgis.md` §3). Open question for the community:
     should a family of map tools (select shapes, measure, graphics) behave like a palette
     instead?
   - At least 3 alternatives in the menu. With 1 or 2, show them as their own (small) buttons:
     a split button then saves at most one slot, but hides the alternative and costs an extra
     click. Exception: Paste | Paste Special, a convention users already know from office
     software (and the ArcGIS Pro SDK's own split-button example).
   - For map tools, the face shows the default even while an alternative tool is active. The
     button must show the family as active, and the tab strip's tool indicator names the tool.
   - Lists with no default use a dropdown (`big_menu`).
6. **Dropdowns:** no menus with a single entry. Keep them to about 15 entries, grouped with
   separators or submenus. Searchable catalogs (Web Data has 33 entries) go in a pane with
   search, not a growing menu.
7. **Preferences go in Settings.** Look and behaviour that applies to every project (dark mode,
   GPU, auto-cache, item pop-ups) is not a working-tab command.
8. **Groups:** avoid groups with a single button; merge them with a neighbour (today: Snapping,
   Corrections, AI, Time, Workflows, Archiving, Table, Definition Query, Learn, Performance). A
   group should not exceed about 8 controls.
9. **Width budget:** each tab should fit on a 1366 px wide window without the scroll arrows,
   checked with the `ui_shot` example.
10. **Labels say what happens.** "…" means a dialog or file picker that asks for input (the same
    rule as the QGIS HIG). Pane buttons show whether the pane is open. A label names the action
    ("Open Project", not "Import Project"). One setting has one name.
11. **Specialist workflow families** (imagery, reports, presentations) get a single entry button,
    a contextual group, or a tab users can hide. They don't get a group per sub-workflow. ArcGIS
    Pro shows layer tabs (Feature Layer, Raster Layer, Labeling) only for a matching selection,
    and its SDK puts "more obscure functions" behind a group launcher. RustGIS does it differently
    since #99: Style and Data are always shown, and their content changes with the selected layer.
12. **New features:** the proposal in CONTRIBUTING.md must state the usage tier, how it opens
    (dialog, pane, tool or immediate), and which existing button it joins or replaces.
13. **Can be enforced:** a test could walk the ribbon with the existing kittest harness and fail
    on any (command, parameters) pair that appears twice and isn't on the allow-list from rule 2.
    It could also generate Command Search's location strings, so they can't go stale. This is a
    suggestion; how hard it would be has not been checked.
14. **Users can trim, not only maintainers.** Hidden tabs and the Quick Access Toolbar already
    exist. ArcGIS Pro also lets users customize groups and QGIS can hide single toolbar icons
    and share the result as a file. Whether RustGIS needs per-group hiding is open.

## 2. Split-button candidates (`big_split_menu`)

These only make sense where the main button has an obvious default, the menu holds variants of
the same action, and there are at least 3 of them (rule 5).

| Button | Default (fixed) | Menu | Alternatives (direct menu entries) |
|---|---|---|---|
| Home › Select (also Edit) | click / box | Polygon · Lasso · Radius (exists since #855) | 3 |
| Home › Add Data | Data… [F] | the rest of today's menu | 15+ |
| Home › New Layout | Letter | 8 page sizes · Publication Layout ▸ · Import Layout… | 10+ |
| Edit › Polygon (graphics) | Polygon | Lasso · Rectangle · Circle · Ellipse | 4 |
| Edit and Layout › Text | Text | Dynamic Text tokens | 7–8 |
| Analyze › Raster Functions | Raster Functions | Function Editor · Indices · Process · Pixel Editor | 4 |
| Layout › Map Frame | Rectangle | Circle · Ellipse · Polygon · Lasso | 4 |
| Layout › Shapes | Rectangle | Circle · Ellipse · Line ▸ · Arrow | 4 |
| Publish › Project Package | Project Package | Map · Mobile Map · Results · Deep Learning | 4 |
| Publish › Web Map | Web Map | 7 other share types | 7 |
| Publish › Save As | Project… | 7 item-file types | 7 |
| Edit › Paste (exception) | Paste | Paste Special | 1 |

Considered and rejected because they had only 1 or 2 alternatives (now separate buttons). Units
and other option values inside a menu don't count as alternatives:

| Candidate | Alternatives | Instead |
|---|---|---|
| Home › Measure | Area (+ units) | Measure Distance and Measure Area as small tool buttons; units move to Settings › Display |
| Edit › Line (graphics) | Curve · Freehand | Line, Curve and Freehand as small tool buttons |
| Data › Export | Export To 3D (the tool entry repeated the Tools gallery) | Export Features [B] · Export To 3D [S] |
| Publish › Export Map | Print Map · Capture To Clipboard | three buttons, as today |
| Layout › Export | Print | Export and Print, as today |
| New Report / Presentation / Notebook | Import… / Open… | New [B] · Import… [S] · pane [S], as today |

Not suitable at all: Bookmarks, Basemap, Styles, Workflows, Primary Symbology and Projection.
They have no default action; they are pickers, so they should stay plain dropdowns
(`big_menu`). Also not suitable: pane buttons with quick actions in the menu (Modify + Editor
Tools, Tasks + Task ▾), and Download Map + Sync/Open/Remove. Their entries are different
actions, not variants of the main action.

## 3. What applying the rules would remove

Applying the rules to the controls in `ribbon-audit.md`:

- **Analyze:** the 8 imagery groups become one Imagery button with its own dropdown (or a
  contextual group shown when a raster is selected). Portal and the second Selection group go.
- **Edit:** Status, Settings and Templates become one Editing pane button. Corrections and
  Elevation fold into it.
- **Data:** the Data Design small buttons become one button. Feature Cache moves to Settings.
- **View:** Windows keeps only general panes (Layers, Browser, Data Sources, Details) plus a
  "Panes ▾" menu listing all panes. Effects and Scene become one dropdown each. Dark Mode, GPU
  and Item Pop-ups move to Settings.
- **Publish:** Thumbnail, Templates and the map import move to the backstage. The split buttons
  for Package, Share As and Save As cut about 17 small buttons.

Those changes would remove about 90 top-level controls (from ~290 to ~200, see
`ribbon-proposal.md`) without removing any feature. That figure is an estimate, not a measurement.

Placement and label fixes from the audit's inconsistencies:

- Share As › Deep Learning saves a package, so it moves to Package.
- Home › "Import Project" becomes "Open Project", or goes (rule 2: the backstage owns Open).
- "Lighting" and "Enable Lighting" become one control with one name (rule 10).
- One label per tool: the Tools gallery and Portal › Feature Analysis use the same name for
  `convexHull` and `countPoints`.
- The two Dynamic Text menus offer the same tokens where they apply.
- Command Search's location strings follow the ribbon (rule 13).

## 4. How ArcGIS Pro and QGIS compare

Details and sources are in `ribbon-arcgis.md` and `ribbon-qgis.md`.

| Question | ArcGIS Pro | QGIS (stock) | QGIS Ribbon Toolbar plugin (0.5, experimental) | Proposal here |
|---|---|---|---|---|
| Split-button default | Last picked becomes the default | Last used (selection, measure); fixed for a few | Fixed for the buttons it defines itself (`BUTTON_MENUS`); cloned QGIS buttons keep QGIS's behaviour | Fixed (#831, implemented by #855) |
| Same command on several tabs | Yes (Selection, labeling) | Menu + toolbar duplicates | Yes (Selection, Toolbox) | Only deliberate mirrors (rule 2) |
| Contextual tabs | Yes (layer, labeling, layout, table, time) | No; tools enable per layer, layouts get their own window | No | No since #99: Style and Data are permanent tabs whose content follows the selected layer; layout groups appear in Publish while a layout is open |
| Long tail | Toolbox, galleries, panes, group launchers | Processing Toolbox, panels | "More" group, hamburger menu with the full QGIS menus | Panes, dropdowns, Command Search |
| Size by frequency | Published rule: frequent = large | No size hierarchy | Large/small, picked by the author's own experience | Rule 4 |
| User customization | QAT, tabs and groups | Hide menus, toolbars and single icons; INI file | None (out of scope) | Hidden tabs, QAT (rule 14) |
| Command search | Yes, includes add-ins | Locator (Ctrl+K) | Uses the QGIS Locator | Command Search (Ctrl+K), but `ui.*` only if listed |
