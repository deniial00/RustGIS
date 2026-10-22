# Actual application size with and without Terrio

These are the real RustGIS applications, not the earlier export harnesses: the macOS desktop app
(`rustgis`, its `.app` and `.dmg`), the CLI (`rustgis-cli`) and the web app (Trunk bundle with its
service worker), built with the project's release settings.

**Terrio adds about 12% to the desktop app and 12% to the web wasm.** It is the Terrio dependency
itself that costs this: keeping the native writer alongside it adds only about 34 KB more.

## Method

- **Isolated integration.** [`../../app-size/terrio-cog-app.patch`](../../app-size/terrio-cog-app.patch)
  is applied to a fresh `git archive` of this checkout's HEAD (`ee149b8`); no production file is
  changed. It adds optional cargo features to `rustgis-engine` and the three apps, and routes the
  engine's real COG export through Terrio: `share.exportRaster` with `"cog": true` (file) and
  `geotiff_bytes` (the browser's download path). The adapter is the evaluation's `adapter.rs`.
  Terrio is `deniial00/terrio@290ce2aa` (the `perf-cog-average-rows` branch, 0.4.0),
  `default-features = false, features = ["geotiff"]`.
- **Three variants.** `baseline`: no Terrio. `replace`: COG export goes through Terrio instead of
  the native writer (the adoption scenario). `alongside`: the native writer stays and Terrio is
  chosen with `"writer": "terrio"` (an upper bound).
- **Lockfile.** Adding Terrio adds 39 packages to `Cargo.lock` (712 → 751) and changes none of
  the existing ones, so all variants build with `--locked` from the same lockfile.
- **Builds.** Desktop: `cargo build --release --locked -p rustgis -p rustgis-cli` (release CI's
  command), then `packaging/package.sh 0.3.0` (ad-hoc signed, not notarized). Web:
  `trunk build --release --public-url /` (Trunk 0.21.14, checksum-verified, as the deploy workflow
  pins; the `web` profile with fat LTO and `wasm-opt -Oz`), then `cargo xtask web-pwa`.
- **Every build** uses a fresh target directory, four Cargo jobs, no incremental compilation,
  dependencies fetched beforehand, sequential execution. Apple M1 Pro, macOS 26.6.2, rustc 1.98.1.
- Run it: `python3 measure_app_size.py --out <fresh dir> --trunk <trunk 0.21.14>`
  (`--resume` continues an interrupted run). Raw numbers, commands, artifact hashes and the patch
  hash are in [results.json](results.json).

## Desktop (macOS arm64)

| | Baseline | Terrio replaces native | Delta | Terrio alongside native |
|---|---:|---:|---:|---:|
| `rustgis` as built | 66.78 MiB | 74.36 MiB | **+7.58 MiB (+11.4%)** | 74.39 MiB |
| `rustgis` stripped (`strip -x`) | 56.19 MiB | 63.08 MiB | +6.89 MiB (+12.3%) | 63.12 MiB |
| `RustGIS.app` bundle | 66.92 MiB | 74.46 MiB | +7.54 MiB (+11.3%) | 74.49 MiB |
| `RustGIS-0.3.0-macos-aarch64.dmg` | 28.73 MiB | 32.05 MiB | **+3.32 MiB (+11.5%)** | 32.05 MiB |
| `rustgis-cli` as built | 41.96 MiB | 49.09 MiB | +7.13 MiB (+17.0%) | 49.12 MiB |
| CLI `.tar.gz` | 15.98 MiB | 18.28 MiB | +2.30 MiB (+14.4%) | 18.29 MiB |
| Clean build, app + CLI | 370.6 s | 461.2 s | +90.7 s (+24.5%) | 435.7 s |

The DMG is what a user downloads. The release build is not stripped, so "as built" is what ships.

## Web

| | Baseline | Terrio replaces native | Delta | Terrio alongside native |
|---|---:|---:|---:|---:|
| wasm, raw | 33.54 MiB | 37.62 MiB | **+4.08 MiB (+12.2%)** | 37.64 MiB |
| wasm, gzip -9 | 11.38 MiB | 12.50 MiB | **+1.12 MiB (+9.8%)** | 12.51 MiB |
| wasm, Brotli 11 | 7.45 MiB | 8.30 MiB | +0.86 MiB (+11.5%) | 8.30 MiB |
| whole site, raw | 33.71 MiB | 37.80 MiB | +4.08 MiB | 37.82 MiB |
| whole site, gzip -9 | 11.41 MiB | 12.53 MiB | +1.12 MiB | 12.54 MiB |
| Clean build (Trunk) | 420.5 s | 452.1 s | +31.6 s (+7.5%) | 450.4 s |

Only the wasm changes materially; the JS glue, HTML and service worker are the same.

**In the browser, this cost buys a path nobody can use yet.** The web app's only export is
`ui.exportRaster`, which calls `geotiff_bytes(&session, &Value::Null)`, so `cog` is never set. The
COG writer is linked (the function takes runtime parameters) but no web user can reach it. Adopting
Terrio in the browser would need a COG option in the web UI to be worth the extra download.

**Scope of the web numbers.** The web app as it ships today contains no Terrio, so its current size
is the baseline column. The Terrio columns are a *prospective integration*: Terrio is wired into
`geotiff_bytes`, which keeps it linked even though the UI never asks for a COG. Fat LTO did not
discard it (the wasm grew by 4.08 MiB), because `cog` is a runtime parameter. Had Terrio been added
only behind a constant the UI never sets, the linker could have removed it; that was not measured.

## What was exercised

- **32 real exports pass** ([smoke.json](smoke.json)). Each variant's shipped `rustgis-cli` opens a
  COG and runs the app's own `share.exportRaster` with `"cog": true` (the `alongside` variant twice:
  default and `"writer": "terrio"`). Eight cases: RGB, Float32 + no-data, UInt16, palette, classes,
  grey + alpha, RGBA and odd dimensions. All 32 outputs pass the evaluation's independent pixel
  oracle and GDAL's full COG validator. Forced BigTIFF was not exercised, because the command picks
  the flavour itself, and the 1027×771 edge case was not repeated here.
- **The Terrio-linked desktop app** launched with `--sample`, stayed up for 10 s and wrote no crash
  log or error output (with an isolated `HOME`, so the user's settings were untouched).
- **The Terrio-linked web app** loaded in headless Chromium with `?webgl`, drew the full UI with no
  page errors, and the loading screen cleared. Its COG export is unreachable from the UI (above),
  so no browser-side COG export was exercised.

## Caveats

- One build per variant, so build times are single samples; `alongside` (435.7 s) building faster
  than `replace` (461.2 s) shows the run-to-run noise. Download and cache effects are not controlled.
- The integration is a measurement snapshot, not production code: it always passes `big = false`
  (the native writer chooses BigTIFF when a file could pass 4 GiB), Terrio's file writer reserves a
  new path instead of replacing atomically, and the browser's file-writing helper is compiled out.
  None of that changes the linked size materially.
- macOS arm64 only. Windows and Linux binaries, signed or notarized packages, and `.deb`/`.rpm`/
  AppImage/MSI sizes are not measured.
- `packaging/package.sh`'s `hdiutil create` failed intermittently with "No space left on device"
  with over 50 GB free, for the Terrio variants. The runner retries the unchanged script; the
  numbers come from the successful runs (`package_attempts` in results.json).
- The first web attempt failed because this shell exports `NO_COLOR=1`, which Trunk rejects; the
  runner now unsets it.
