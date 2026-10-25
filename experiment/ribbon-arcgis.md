# ArcGIS Pro ribbon: selective exposure, contextual depth

ArcGIS Pro combines task-oriented tabs with selection-dependent tabs, galleries, and dockable panes; its ribbon is not the only route to functionality. Its SDK explicitly gives frequent commands visual priority and sends obscure options behind group launchers. Crucially, Esri's generic **split button promotes the selected command**, rather than guaranteeing a permanently fixed default action. [Introduction][intro] · [Ribbon SDK guide][ribbon-sdk] · [Split-button semantics][framework-split]

## 1. Structure

### Core tabs and context

- The requested baseline tab inventory is **Project, Map, Insert, Analysis, View, Edit, Imagery, Share, Help**. Esri's 3.1 tutorial explicitly lists the eight working tabs from Map through Help; Project is the separate entry to application/project settings, not an ordinary command-group tab. [Explicit tab list, 3.1][intro-31] · [Current Project/settings documentation][intro]
- Do not interpret “core” as “always visible in every view”: current help says core tabs may be always present or dependent on the active view type; Help is always present and Map appears with an active map. [Current interface overview][intro]
- Switching to a layout replaces **Map** with **Layout** and removes **Edit** in the official tutorial. Layout is therefore a view-dependent working tab, not merely a layer-selection tab. [Current tutorial, Modify a layout][tutorial]
- Selecting a feature layer in Contents reveals **Feature Layer**, **Labeling**, and **Data** contextual tabs. These supplement the map workflow rather than requiring every layer command on Map. [Current interface overview][intro]
- **Raster Layer** is likewise conditional: published DAML associates its tab with `esri_mapping_onlyRasterLayersSelectedCondition`. [Mapping DAML][mapping-daml]
- **Table** appears for an active table view; **Time** appears when a map layer has time enabled. User-created contextual tabs must accompany an existing contextual tab. [Customization help][customize]

### Groups, extensions, and backstage

- Groups organize related commands within tabs; a small dialog launcher opens a related pane or dialog. Concrete groups include Map → Navigate, Selection, and Inquiry. [Interface overview][intro] · [Map group declarations][mapping-daml]
- SDK add-ins can introduce tabs, contextual tabs, groups, controls, dock panes, and backstage tabs. A group with `appearsOnAddInTab="true"` is placed on the **Add-In** tab without requiring a new tab declaration. [Framework extensibility and Tab and group sections][framework]
- Add-ins can instead extend an existing group: Esri shows `updateModule` → `updateGroup` → `insertButton`, with `insert="before"` and `placeWith` to position a command relative to an existing one. [Framework DAML introduction][framework]
- Terminology matters: Pro's **plugin datasource** is a headless extension and cannot contain DAML buttons; ribbon extensions are **add-ins**. Do not equate its narrow “plugin” term with RustGIS's general plugin concept. [Framework, Plugin Datasource][framework]
- **Project** opens the settings/backstage equivalent. Its side tabs include New, Open, Info, Save Project, Save Project As, Portals, Licensing, Options, Package Manager, Add-In Manager, Help, About, Learning Resources, and Exit. [Introduction, Settings table][intro]
- The same settings are reachable from the start page's **Settings** tab. The SDK explicitly names this application-management surface **Backstage**. [Introduction][intro] · [Framework, Backstage][framework]

### Quick Access Toolbar, customization, and search

- The Quick Access Toolbar (QAT) sits above the ribbon and defaults to **New Project, Open Project, Save Project, Undo, Redo**. New/Open/Save are also available through settings. [QAT help][qat]
- Users can add/remove/reorder QAT commands, right-click a ribbon command to add it, and import/export customization. Esri's example adds Insert → New Map to the QAT. [QAT help][qat]
- Users can create tabs/groups, add commands or geoprocessing tools, rename custom structures, reorder groups and commands in custom groups, and hide built-in or custom tabs. Built-in tabs cannot be removed, and built-in-group command ordering cannot be changed. [Customization help][customize]
- New custom tabs default to the right of Help; the help explicitly discourages moving them because their position may not behave as expected in all situations. Custom contextual tabs retain their parent's context requirement. [Customization help][customize]
- Customizations can be exported to `.proExportedUI`; importing overwrites existing customizations. Reset supports a selected built-in tab or the ribbon and QAT together. [Customization help][customize]
- **Command Search** at the top of an open project finds/runs commands, opens geoprocessing tools and raster functions, and offers help. It presents recent and suggested commands and disables commands inappropriate to the current state. [Command Search help][search]
- Searching “split” can directly open the Modify Features pane with Split selected, bypassing manual ribbon/pane navigation. [Command Search example][search]
- Add-in buttons/tools are automatically discoverable through their DAML ID, caption, or tooltip unless excluded with `hidden="true"`. [Framework, Command search][framework]

## 2. Control types and their actual semantics

### Big and small command buttons

- Published rule: “**Frequently used controls should be large and obvious, while less frequently used tools should be smaller.**” Captions normally help explain function. [Ribbon SDK overview][ribbon-sdk]
- Size is a property of a control's placement in a group, not just of its command definition: the same command can be large in one group and small in another. [Framework, Tab and group][framework]
- Esri's Navigate example places **Explore** large and **Full Extent**, **Fixed Zoom In/Out**, and **Previous/Next Extent** small. Its Labeling example places the layer labeling button large. [Framework, Tab and group][framework]
- Group sizing can adapt to window width; `sizePriorities` controls which groups shrink first, and subgroups constrain how related controls scale together. [Framework, Scaling controls using sizePriorities][framework] · [Ribbon SDK subgroups][ribbon-sdk]
- **Create** and **Modify** are ordinary buttons in the published Editing DAML, opening Create Features and Modify Features panes. They are not evidence that every pane opener is a split button. [Editing DAML, control definitions][editing-daml]

### Split buttons: initial default versus selected action

- The generic SDK split button starts with the **first listed button**, which “should be the most commonly used button”; the arrow exposes the associated collection. [Framework, Split button][framework-split]
- Its documented behavior is explicit: “**Once a control is selected from the drop-down list, it becomes the active control in the split button.**” The separate Palettes and Split Buttons guide repeats this rule. [Framework, Split button][framework-split] · [Palette guide][palettes]
- Thus this is a **selected/last-chosen action** model during interaction, not RustGIS's proposed invariant “main face always executes the same fixed default.” This distinction follows directly from the SDK contract; persistence across application restarts is **not established** by these sources. [Framework, Split button][framework-split] · [Palette guide][palettes]
- Generic split buttons may combine buttons with menus, dynamic menus, or galleries; Esri's example is Paste plus alternative Paste commands or a Paste gallery. The example names beginning `acme_` are SDK illustrations, not claims about shipped UI labels. [Framework, Split button][framework-split]
- Concrete shipped declarations include **Add Data** (`esri_mapping_addDataSplitButton`: Add Data button + inline dynamic menu), **Symbology** (pane-opening button + symbology gallery), and **Append** on Animation (append-transition button + transition alternatives). [Mapping DAML, splitButtons][mapping-daml]
- **Measure** is also declared as `esri_mapping_measureSplitButton`, containing a “current tool” button and a custom tools-menu control. User help places it at Map → Inquiry → Measure, with distance, area, features, vertical, direction-distance, offset, and angle alternatives. [Mapping DAML][mapping-daml] · [Measure help][measure]
- **Evidence boundary:** Measure's custom menu/current-tool implementation is not supplied by its DAML declaration. The generic SDK rule does not establish precisely how that custom control persists the last tool, especially across sessions. Do not report that persistence as an observed fact. [Mapping DAML][mapping-daml] · [Framework, Split button][framework-split]

### Tool palettes and split-looking tools

- **Select** on Map is a **tool palette**, not a DAML `splitButton`: the palette lists Rectangle first, followed by Polygon, Lasso, Circle, Line, Trace, Box, Sphere, and Cylinder. [Mapping DAML, selectToolPalette][mapping-daml]
- For tool palettes, selecting an item makes it the active tool and displays it on the collapsed palette; the first listed tool is the initial display. This directly establishes changing-current-tool behavior for the Select family, rather than a permanently fixed Rectangle action. [Palette guide, Tool palette][palettes] · [Mapping DAML][mapping-daml]
- Button palettes similarly execute a chosen button and show it on the collapsed palette; the guide uses **Link Views** with Center versus Center And Scale. [Palette guide, Button palette][palettes]
- **Explore** is documented as a “split tool”: its top half performs identification, while the bottom opens choices that adjust Explore's behavior; Explore also supports panning/zooming. This is an action-plus-behavior-options example, distinct from claiming every arrow selects a replacement command. [Current tutorial, Explore the map][tutorial]

### Menus, galleries, toggles, and combos

- A DAML menu is a declarative list that can contain buttons, dynamic menus, galleries, and nested menus. Unlike a promoted-action control, its documented purpose is to present those choices. [Framework, Menu][framework]
- Concrete menu: **More** in Map → Labeling or Labeling → Map exposes priorities, weights, engine choice, and—under Maplex—abbreviation dictionaries, key numbering, and draft quality. [Labeling basics][labeling]
- Galleries present related items/commands with richer previews, multiple rows/columns, and expansion for additional choices. They can be dropdown or inline; inline items may be most-common or recent depending on implementation. [Framework, Gallery][framework]
- Concrete galleries: **Basemap** is a dropdown preview gallery; **Layer Templates** is the framework's inline example. **Analysis → Tools** shows common spatial-analysis tools, expands/scrolls, and can be customized for a project's workflow. [Framework, Gallery][framework] · [Find geoprocessing tools][geoprocessing]
- Imagery → Tools contains **Process** and **Indices** galleries, alongside entry points to Pixel Editor and Image Information panes. [Imagery Tools group][imagery]
- Checked buttons/checkboxes expose state, while tools stay checked after selection. Labeling provides the concrete **Label Features In This Class** checkbox and layer **Label** on/off control. [Framework, Controls/Tool][framework] · [Labeling basics][labeling] · [Mapping DAML][mapping-daml]
- Combos select a value and can optionally permit text entry. Concrete ribbon examples are Labeling's **Font Size** and **Font Style** combos; the published DAML makes Font Size editable. [Framework, Combo box][framework] · [Ribbon SDK, Toolbars][ribbon-sdk] · [Mapping DAML][mapping-daml]
- [INFERENCE] A useful taxonomy is **command** (button), **repeat current member of a family** (palette/promoting split button), **choose among commands** (menu), **compare visual results** (gallery), and **set persistent state/value** (toggle/combo). This interpretation combines the cited control contracts, rather than quoting an Esri taxonomy. [Framework][framework] · [Palette guide][palettes]

## 3. How specialist functionality avoids permanent ribbon exposure

- **Toolboxes and search:** all geoprocessing system tools, including extension/industry toolboxes, are organized under Geoprocessing → Toolboxes → System. Users also have pane search, Favorites, Recent, project toolboxes, and Catalog access. [Find geoprocessing tools][geoprocessing]
- **Curated front door:** Analysis → Tools highlights commonly used analysis operations instead of requiring every system tool to become an individual visible button. Its contents can be customized for a project. [Find geoprocessing tools][geoprocessing]
- **Pane depth:** basic label size/color are on Labeling, while advanced properties are in Label Class, reached through group launchers, Expression, SQL Query, or the layer context menu. [Labeling basics][labeling]
- **Specialist galleries/panes:** Process and Indices collect imagery functions; Pixel Editor and Image Information consolidate deeper raster operations. [Imagery Tools group][imagery]
- **Selection context:** Feature Layer/Labeling/Data and Raster Layer appear for suitable layer selections, and Table/Time have active-view or layer-property conditions. [Introduction][intro] · [Customization help][customize] · [Mapping DAML][mapping-daml]
- **Layout context:** the layout workflow changes the main working tab; legend border and symbol-layer details are edited in the Element pane rather than assigned individual permanent ribbon buttons. [Current tutorial, Modify a layout][tutorial]
- **Obscure settings:** the SDK explicitly permits a group “more”/launcher button for “more obscure functions.” [Ribbon SDK, How to add a popup][ribbon-sdk]
- **Not proven search-only:** these sources establish search as an additional route, not a policy of intentionally making some GIS commands discoverable only through search. Add-in search participation is documented independently of the command's visual placement. [Find geoprocessing tools][geoprocessing] · [Framework, Command search][framework]

## 4. Deliberate duplication

- **Yes—same functionality on different tabs:** Pause, Lock, and View Unplaced labeling tools are available both in **Map → Labeling** and **Labeling → Map**. The More menu is also accessible in either place. [Labeling basics, Labeling properties at map level][labeling]
- **Same command identities, not just similar captions:** Map's Selection group and Edit's Selection group reference `esri_mapping_selectToolPalette`, `esri_mapping_clearSelectionButton`, and `esri_mapping_zoomToSelectionButton`. Editing DAML also inserts the same Attributes command into Map's Selection group. [Mapping DAML][mapping-daml] · [Editing DAML][editing-daml]
- **Across surfaces:** New/Open/Save Project appear on the QAT and settings; users can also duplicate ribbon commands onto the QAT. [QAT help][qat]
- **Important exception:** matching names do not guarantee matching commands. Customization help gives separate **Zoom Full Extent** commands for maps, metadata previews, presentations, and network diagrams; command IDs disambiguate them. [Customization help, Search using command IDs][customize]
- [INFERENCE] Pro permits duplication when it serves a task context; it does not enforce a universal “one command, one location” rule. The cited examples establish that practical pattern, not a published blanket recommendation to duplicate everything. [Labeling basics][labeling] · [Editing DAML][editing-daml]

## 5. Published rules for add-in authors

- **Visual priority:** “Frequently used controls should be large and obvious”; less-frequent tools should be smaller. [Ribbon SDK overview][ribbon-sdk]
- **Progressive disclosure:** group launchers open dialogs containing “more obscure functions.” [Ribbon SDK, How to add a popup][ribbon-sdk]
- **Context instead of near-duplicate tabs:** conditional groups “should prevent the need for creating multiple tabs that look nearly identical.” A tab condition controls availability, not activation. [Framework, Tab Conditions][framework]
- **Contextual group consistency:** “Tabs in a tab group should have the same condition”; tab groups are generally for temporary modes, not perpetually visible global tabs. [Framework, Tab and group][framework]
- **Default selection:** the first split-button command “should be the most commonly used button.” Selection then promotes a different active control. [Framework, Split button][framework-split]
- **Integration mechanisms, not placement mandates:** `appearsOnAddInTab`, custom tabs, and insertion into existing groups are documented alternatives. These sources do **not** prescribe “every add-in gets its own tab” or a numerical button/tab limit. [Framework, Tab and group and DAML introduction][framework]
- **Accessibility:** supply meaningful, nonconflicting tab keytips; multiple characters can disambiguate. [Framework, Tab Conditions][framework]
- **Consistent appearance:** “ArcGIS Pro XAML styles should be used as DynamicResources”; built-in styles provide light/dark-theme variants, and custom windows should derive from ProWindow. This is styling guidance, not a ribbon-placement policy. [Pro Style Guide][style]
- **Supported implementation path:** Esri recommends palette/split-button item templates instead of manually following the declaration guide. [Palette guide introduction][palettes]

## 6. Lessons for RustGIS

Each recommendation below is **[INFERENCE]**, grounded in the linked Pro behavior rather than presented as an Esri requirement for RustGIS.

1. **Give large buttons to frequent workflow entry points, not every capability.** Use smaller controls and group launchers for lower-frequency options; this directly follows Pro's published frequency rule. [Ribbon SDK][ribbon-sdk]
2. **Specify fixed-default and last-used controls separately.** RustGIS's proposed fixed action + alternatives is not Esri's generic promoting split-button contract. Label and test those semantics explicitly; do not let one silently become the other. [Split-button contract][framework-split]
3. **Use a current-tool palette for selection geometries.** Rectangle/Polygon/Lasso are alternatives in one tool family; Pro makes the selected member active and visible. [Palette guide][palettes] · [Select declaration][mapping-daml]
4. **Use a menu for behavior/settings choices, not necessarily action promotion.** Explore's identification behavior choices and Labeling's More menu illustrate two different reasons for an arrow. [Tutorial][tutorial] · [Labeling basics][labeling]
5. **Expose visual collections as galleries.** Basemaps, templates, and curated analysis tools benefit from preview or compact collection presentation; don't flatten each item into a top-level button. [Framework galleries][framework] · [Analysis gallery][geoprocessing]
6. **Make specialist tabs conditional on meaningful context.** Feature-layer selection, raster-layer selection, time-enabled data, and active layout/table views are concrete eligibility signals. [Introduction][intro] · [Customization][customize] · [Mapping DAML][mapping-daml]
7. **Keep the long tail in searchable toolboxes and focused panes.** Pro's geoprocessing inventory and advanced labeling pane demonstrate depth without a permanent button per operation. Preserve browse access as well as search. [Geoprocessing discovery][geoprocessing] · [Labeling basics][labeling]
8. **Allow intentional duplication, but share command identities.** Put selection and labeling commands where their workflows need them, as Pro does, while avoiding separate implementations hidden behind identical labels. [Mapping DAML][mapping-daml] · [Editing DAML][editing-daml] · [Command-ID guidance][customize]
9. **Give users an escape hatch before adding global surface area.** A customizable QAT, hideable tabs, custom groups, and command search let specialists promote their own commands instead of imposing them on everyone. [QAT][qat] · [Customization][customize] · [Search][search]
10. **Offer extensions an integration hierarchy rather than automatic tab sprawl.** Support existing-group insertion, a shared add-in area, and genuinely contextual tabs; Pro demonstrates all three mechanisms without mandating a new global tab for each extension. [Framework][framework]

## 7. Sources and evidence scope

Primary sources only. Help pages were read in their current **3.7** form; the **3.1** tutorial is explicitly retained for its textual eight-tab enumeration. SDK wiki pages identify ArcGIS Pro **3.7**; repository `master` declarations are implementation evidence and may evolve independently of installed releases. No desktop interaction was performed, and no cross-restart memory behavior was observed. [Current help version][intro] · [Versioned tab list][intro-31] · [SDK guide version][ribbon-sdk] · [DAML repository][mapping-daml]

- [Introduction to ArcGIS Pro: ribbon, contextual tabs, panes, Project/settings][intro]
- [Introducing ArcGIS Pro: current tutorial, Explore split tool, layout switch][tutorial]
- [Introducing ArcGIS Pro 3.1: explicit core-tab enumeration][intro-31]
- [Customize the ribbon options][customize]
- [Quick Access Toolbar options][qat]
- [Find tools and help using Command Search][search]
- [SDK ProGuide: The Ribbon, Tabs and Groups][ribbon-sdk]
- [SDK ProConcepts: Framework][framework]
- [SDK ProConcepts: Framework — Split button][framework-split]
- [SDK ProGuide: Palettes and Split Buttons][palettes]
- [SDK ProGuide: Style Guide][style]
- [Esri SDK repository: Mapping DAML][mapping-daml]
- [Esri SDK repository: Editing DAML][editing-daml]
- [Measure][measure]
- [Labeling basics][labeling]
- [Find a geoprocessing tool][geoprocessing]
- [Tools group on the Imagery tab][imagery]

[intro]: https://doc.esri.com/en/arcgis-pro/latest/get-started/get-started.html
[tutorial]: https://doc.esri.com/en/arcgis-pro/latest/get-started/introducing-arcgis-pro.html
[intro-31]: https://pro.arcgis.com/en/pro-app/3.1/get-started/introducing-arcgis-pro.htm
[customize]: https://doc.esri.com/en/arcgis-pro/latest/get-started/customize-the-ribbon.html
[qat]: https://doc.esri.com/en/arcgis-pro/latest/get-started/quick-access-toolbar.html
[search]: https://doc.esri.com/en/arcgis-pro/latest/get-started/find-tools-and-help-command-search.html
[ribbon-sdk]: https://github.com/Esri/arcgis-pro-sdk/wiki/ProGuide-Ribbon-Tabs-and-Groups
[framework]: https://github.com/Esri/arcgis-pro-sdk/wiki/ProConcepts-Framework
[framework-split]: https://github.com/Esri/arcgis-pro-sdk/wiki/ProConcepts-Framework#split-button
[palettes]: https://github.com/Esri/arcgis-pro-sdk/wiki/ProGuide-Palettes-and-Split-Buttons
[style]: https://github.com/Esri/arcgis-pro-sdk/wiki/ProGuide-Style-Guide
[mapping-daml]: https://github.com/Esri/arcgis-pro-sdk/blob/master/DAML/ADMapping.daml.xml
[editing-daml]: https://github.com/Esri/arcgis-pro-sdk/blob/master/DAML/Editing.daml.xml
[measure]: https://doc.esri.com/en/arcgis-pro/latest/help/mapping/navigation/measure.html
[labeling]: https://doc.esri.com/en/arcgis-pro/latest/help/mapping/text/labeling-basics.html
[geoprocessing]: https://doc.esri.com/en/arcgis-pro/latest/help/analysis/geoprocessing/basics/find-geoprocessing-tools.html
[imagery]: https://doc.esri.com/en/arcgis-pro/latest/help/data/imagery/tools-group-on-imagery-tab.html
