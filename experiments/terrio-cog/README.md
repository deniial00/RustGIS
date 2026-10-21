# Terrio COG integration experiment

**Recommendation: retain the native RustGIS writer. Do not adopt this adapter in production.**
The feature split makes an isolated experiment practical, but a structurally valid COG is not
necessarily a semantically equivalent export. The measured desktop comparison passes GDAL's
full COG validator for all 18 files; Terrio fails six of the nine pixel/overview comparisons.

This is the separate experiment requested in
[RustGIS #629](https://github.com/opengeos/RustGIS/pull/629#issuecomment-6301598478), not a writer
replacement. No production source, production manifest, production lockfile, UI, or other I/O
was changed. No PR or upstream changes were published.

## Isolation and revision

This directory and `probe/` are independent Cargo workspaces with committed lockfiles.
They are not members of the RustGIS production workspace. This package defaults to the native
writer only; `--features terrio` explicitly links the experimental adapter. Normal RustGIS
builds neither resolve nor compile this dependency.

Terrio is pinned to **`ce3c4463f19864284d743720123c4d022784c497`**, versioned upstream as 0.3.2,
with `default-features = false, features = ["geotiff"]`. This is the inspected main revision:
[feature merge #154](https://github.com/opengeos/terrio/pull/154) followed by
[CRS fix #155](https://github.com/opengeos/terrio/pull/155). A floating `0.3` constraint would
not establish that the feature merge is present. The original
[feature request #139](https://github.com/opengeos/terrio/issues/139) is closed by #154.

The actual locked minimal normal-dependency graphs contain **118 native** and **107 wasm**
package/version identities, excluding the probe itself, including normal proc-macro dependencies.
The `geotiff` feature is empty: GeoTIFF is always-on core, not an independently gated writer.
No Arrow, Parquet, netcdf-reader, hdf5-reader, OpenJPEG, AV1, LERC, LAZ, or remote HTTP client
is in these probe graphs. JPEG XL, both WebP implementations, wbvector, wbprojection,
**wbhdf**, three zip versions, and multiple compression versions remain. In particular,
removing hdf5-reader does not remove the transitive wbhdf parser. See
[`results/dependencies.json`](results/dependencies.json) for the exact packages and isolation check.
Both minimal native and `wasm32-unknown-unknown` probe builds succeeded without special
allocators or `--allow-undefined`.

## Runnable adapter and scope

- `src/adapter.rs`: validated, lossless *sample conversion* from a borrowed RustGIS `Raster`
  to Terrio's owned representation, then Terrio's public COG writer. This does not imply
  lossless overview semantics; known upstream mismatches are deliberately exposed.
- `src/fixtures.rs`: deterministic planar-f32 RustGIS inputs; no file decoding in export timings.
- `src/main.rs`: one real desktop file export per process, JSON timing/size output.
- `src/lib.rs`: the native comparator plus a real wasm-bindgen `export_case` entry point.
- `compare.py`: independent NumPy pixel oracles, GDAL full COG validation, metadata checks,
  bounded TIFF directory inspection, and process-isolated performance measurements.
- `smoke_wasm.cjs`, `verify_wasm.py`: execute actual wasm exports in Node and apply the same
  independent GDAL checks to their output files.
- `measure_build.py`: clean-target builds, linked binary/wasm sizes, and source/artifact hashes.

Accepted adapter storage is explicit UInt8, UInt16, or Float32, with an exactly representable
no-data value. It supports a selected scalar band, RGB, RGBA, grey+alpha, and opaque indexed
palettes. Inputs require recognized EPSG coordinates and finite north-up, nonempty bounds.
Dimensions, every band length, selected band, required colour bands, multiplication overflow,
sample count, integer representability, and no-data representability are checked before Terrio.
No integer rounding/clamping, f64 image intermediary, or silent float fallback occurs.

**Explicitly unsupported by this adapter:** other storage types; integer NaN/fractional/out-of-range
samples; infinite float samples; nonrepresentable no-data; nonopaque palettes; custom/non-EPSG
CRSs; south-up/negative-x grids; malformed shapes or missing bands. The fixture API additionally
caps total samples at the lesser of RustGIS's platform limit and 67,108,864 (32,000,000 on wasm).
The native writer's broader fallback behaviour is not changed or claimed as adapter coverage.

The desktop experiment writes only new generated paths. Terrio's path-only public writer
reopens a reserved path, so this is **not** the production writer's transactional replacement
contract, nor a race-free API for untrusted shared directories. Failed experimental exports may
leave partial files. Use a private evaluation output directory.

### Full-image allocation audit

The native `cogwrite::write_to` borrows RustGIS's planar f32 bands, uses bounded tile workspaces,
and makes each overview directly from the full image. It does not copy the full image or retain
a full overview pyramid. The desktop comparison streams it to a 1 MiB `BufWriter<File>`.

The Terrio adapter makes **one full-image typed conversion buffer**: one byte, two bytes, or
four bytes per selected sample. RustGIS's source f32 bands remain alive. There is no additional
adapter sample clone. This copy is required by the chosen public owned-raster API, not an
intrinsic requirement of all possible COG designs.

`wbraster 0.2.1::Raster::from_data_native` moves its argument, but first calls `Raster::new`,
allocating/filling an entire throwaway typed buffer before replacing it. The adapter avoids
that discarded allocation by constructing the public Raster fields after validating them.
This public-field coupling is intentional and confined to the pinned experiment.

Terrio borrows the converted full image internally, but retains successively allocated
full overview levels and prepared compressed chunks. Tile compression can also run in parallel.
Thus one adapter copy does **not** mean constant additional memory. Its public
`GeoTiffWriter` row API explicitly rejects COGs/overviews; the streaming COG machinery is internal.
Native `memory::write_raster_bytes` also writes a temporary file and reads it back. Desktop
measurements therefore call the direct file API, not that memory path. Wasm uses the real
in-memory codec, retaining returned TIFF bytes; wasm-bindgen/JS transfer copies are not included
in the desktop RSS figures.

Source evidence at the pin:
[COG preparation and overview retention](https://github.com/opengeos/terrio/blob/ce3c4463f19864284d743720123c4d022784c497/crates/terrio/src/drivers/gtiff_write.rs#L695-L740),
[full-image borrow/downsample](https://github.com/opengeos/terrio/blob/ce3c4463f19864284d743720123c4d022784c497/crates/terrio/src/drivers/gtiff_write.rs#L843-L858),
[public row-writer limitation](https://github.com/opengeos/terrio/blob/ce3c4463f19864284d743720123c4d022784c497/crates/terrio/src/drivers/gtiff_write.rs#L1383-L1447),
[native memory path](https://github.com/opengeos/terrio/blob/ce3c4463f19864284d743720123c4d022784c497/crates/terrio/src/memory.rs#L112-L135).
The exact constructor evidence is in the locked registry source `wbraster-0.2.1/src/raster.rs`,
lines 1040–1057 and 1084–1101.

## Correctness methodology and desktop findings

Inputs/settings are identical: EPSG:32617; origin `(500000,4000000)`; pixel size `(30,-30)`;
512×512 tiles; pixel interleaving; DEFLATE effort 6; predictor 1; automatic power-of-two overviews
until the image fits one tile. Categorical rasters request nearest; continuous rasters request
average. **Terrio ignores its public COG resampling request**, so the effective categorical
algorithm is not equivalent. The experiment does not precompute replacement overviews or hide
that mismatch. DEFLATE implementations differ: native miniz_oxide versus Terrio's resolved
flate2/zlib-rs backend. Compression level is established from source, not inferred from TIFF tags.

RGB uses 1024×768; the other correctness cases use 1025×769. Every full-resolution pixel,
every overview pixel (including clipped odd edges), stored type, colour interpretation,
geotransform, EPSG identity, no-data/validity, palette, TIFF flavour, compression, predictor,
tile size, and overview dimensions is independently checked. The reference overview is each
clipped full-source block's average, excluding no-data and alpha-weighting colour channels;
classes take the full-source block center. Means use f64 then f32 and round integer samples
half-up, matching the native contract. Float32 comparisons are exact; reported error magnitude
distinguishes significant validity-weight errors from rounding differences.

| Case | Native | Terrio | Measured distinction |
|---|---|---|---|
| RGB, even dimensions | Pass | Pass | Every sample and overview matches |
| Float32 with numeric no-data | Pass | Fail | Second overview: 2,501 mismatches, maximum absolute error 11.34166 |
| UInt16 with no-data | Pass | Fail | Second overview: 2,585 mismatches, maximum absolute error 2,977; full UInt16 values retained |
| Opaque palette | Pass | Fail | Palette retained, but overview averaging invents class indexes |
| Classes `{1,7,19}` | Pass | Fail | First overview pixel should be 19, is 9; 181,568 overview mismatches total |
| Grey+alpha | Pass | Fail | Full bands/tags retained; 246,528 colour overview mismatches |
| RGBA / alpha-weighted colour | Pass | Fail | Expected overview RGB `(200,100,50)`, actual `(100,50,25)`; 739,584 colour mismatches |
| Odd RGB dimensions | Pass | Pass | Ceil overview dimensions and edge pixels match |
| Forced BigTIFF | Pass | Pass | Real 64-bit TIFF header, COG layout and samples verified |

All 18 outputs pass GDAL full COG validation **without warnings**. Both writers preserve all
full-resolution samples and checked metadata. The failures above are overview semantics,
not missing CRS tags, lost UInt16 storage, missing alpha bands, or invalid COG structure.
The Float32/UInt16 discrepancy appears when recursive averages lose the original valid-support
weights; e.g. Float32 overview level 2 starts at 3.0833333 instead of 3.2.
Palettes and classes each have 181,568 overview sample mismatches. The adapter does not claim
those exports as production-compatible merely because the calls return successfully.

Evidence: [`results/desktop.json`](results/desktop.json). `status: FAIL` and exit code 1 are
intentional measured incompatibilities, not an incomplete run or suppressed validator errors.
The performance records' `PASS` means the process/protocol/RSS measurement succeeded; it does
not upgrade a semantically failing export to a correctness pass.

**Coverage limits:** one EPSG grid, deterministic synthetic inputs, opaque byte palettes, selected
sample types, forced small BigTIFF. Files beyond 4 GiB, automatic threshold crossing, palette
transparency, arbitrary CRS definitions, lossy codecs, reprojecting, remote I/O, and a browser UI
are not evaluated. Wasm execution uses Node, not a browser; this proves wasm module linkage and
exports, not browser download UI behaviour.

## Reproduce

Run from this directory. Prerequisites: Rust with `wasm32-unknown-unknown`, a clang supporting
wasm C compilation, Python with matching GDAL bindings/`osgeo_utils` and NumPy, Node, brotli,
and **wasm-bindgen CLI 0.2.129**, matching the pinned wasm-bindgen dependency. On macOS the build
runner uses Homebrew LLVM's clang/llvm-ar; elsewhere set `CC_wasm32_unknown_unknown` and
`AR_wasm32_unknown_unknown` if needed. No allocator shim or undefined-symbol flag is needed.

```sh
# First inspect/prove the minimal dependency, independently of RustGIS.
cargo tree --locked --manifest-path probe/Cargo.toml -e normal --prefix none
cargo tree --locked --manifest-path probe/Cargo.toml -e normal --prefix none --target wasm32-unknown-unknown
cargo build --locked --manifest-path probe/Cargo.toml
cargo build --locked --manifest-path probe/Cargo.toml --target wasm32-unknown-unknown

# Distinct, clean target directories; builds must run sequentially for timing.
python3 measure_build.py native-baseline --out .local/repro
python3 measure_build.py native-terrio --out .local/repro
python3 measure_build.py wasm-baseline --out .local/repro
python3 measure_build.py wasm-terrio --out .local/repro

# Real desktop exports, independent oracle, full GDAL validator and RSS/time/size.
python3 compare.py --binary .local/repro/native-terrio --out .local/repro/desktop --repeats 3 --size 4096
# Above exits 1 after completing all runs if the pinned incompatibilities remain.
python3 compare.py --binary .local/repro/native-baseline --writers native --phase performance --out .local/repro/baseline-performance --repeats 3 --size 4096

# Actual wasm exports, then the identical independent correctness checks.
node smoke_wasm.cjs .local/repro/wasm-baseline/eval.js .local/repro/wasm-baseline-exports native
node smoke_wasm.cjs .local/repro/wasm-terrio/eval.js .local/repro/wasm-terrio-exports both
python3 verify_wasm.py .local/repro/wasm-baseline-exports
python3 verify_wasm.py .local/repro/wasm-terrio-exports
# Last command exits 1 for measured semantic failures, retaining verification.json.

cargo test --locked --release --features terrio --lib
cargo clippy --locked --release --features terrio --all-targets -- -D warnings
cargo fmt --check
```

`--out` correctness/performance directories and wasm smoke directories must be new. Build
measurement refuses an existing target directory rather than accidentally timing an incremental
build. Choose a new `.local/repro-*` directory for another run. TIFFs, binaries, build trees,
compressed wasm and raw local logs are ignored; machine-readable measured results are retained
in `results/`. A single export is also directly runnable:

```sh
cargo run --locked --release --features terrio -- export terrio uint16 1025 769 .local/new-uint16.tif
cargo run --locked --release --features terrio -- export native bigtiff 1025 769 .local/new-big.tif big
```

## Measurement contract

Host: Apple M1 Pro, 8 logical CPUs, 16 GiB RAM, macOS 26.6.2; rustc 1.98.1
(`48a229cea`, LLVM 22.1.8); GDAL 3.13.3; Python 3.15.0; NumPy 2.5.4; Node 24.20.0;
wasm-bindgen 0.2.129; brotli 1.2.0. No CPU isolation or cold filesystem-cache control is claimed.

Desktop performance uses 4096×4096 RGB and Float32/no-data fixtures. Each table value is the
median of three fresh processes. The combined binary alternates native then Terrio per repetition;
a native binary built without the Terrio feature (still linking all of `rustgis-data`) is measured separately as a baseline. Export time excludes fixture
construction but includes conversion, compression, file output, final `sync_all`, and metadata lookup.
RSS is `/usr/bin/time -l` maximum resident set size in bytes, including fixtures and all writer
allocations, **not** incremental heap or wasm/browser memory. No warmup run is discarded.
Terrio uses its default Rayon parallelism; native is serial. Thread environment is recorded.
These are synthetic, losslessly compressed inputs, not a representative corpus or a throughput
claim for all GIS rasters. Semantic differences particularly invalidate treating the Float32 speed
as the speed of an equivalent result.

Clean builds use four Cargo jobs, incremental off, separate initially absent target directories,
pre-fetched locked dependencies, warm OS/Cargo source caches, and sequential runs. Download time
is excluded; build times are one sample per variant, not medians or confidence intervals. Native
uses the RustGIS release settings (opt3, thin LTO, 16 codegen units); wasm uses the RustGIS web
settings (opt3, fat LTO, one codegen unit). No wasm-opt pass is applied. Wasm sizes are measured
**after wasm-bindgen**; gzip uses level 9 and mtime 0, Brotli quality 11. JS glue is recorded
separately and not included in compressed wasm. Native sizes include both linked and `strip -x`
measurements.

**Size/build baseline scope:** these are real linked desktop export executables and callable wasm
export modules containing the RustGIS Raster/native writer, with and without the Terrio path.
They are not the full RustGIS GUI executable or web application's total download. This deliberately
isolates writer integration without changing production apps. LTO and shared dependencies mean
these deltas must not be added arithmetically to production application sizes. No full-application
size or build-time claim is made.

## Measured results

Desktop export (4096×4096, median of three fresh processes, final frozen binaries):

| Fixture | Writer | Export ms | Peak RSS MiB | Output MiB | Benchmark output semantics |
|---|---|---:|---:|---:|---|
| RGB UInt8 | native, without Terrio | 1173.8 | 201.8 | 21.88 | Pass |
| RGB UInt8 | native, combined binary | 1254.8 | 202.3 | 21.88 | Pass |
| RGB UInt8 | Terrio | 326.5 | 323.1 | 20.64 | Pass |
| Float32 + no-data | native, without Terrio | 405.1 | 73.9 | 1.55 | Pass |
| Float32 + no-data | native, combined binary | 411.8 | 74.4 | 1.55 | Pass |
| Float32 + no-data | Terrio | 84.8 | 194.8 | 3.01 | **Fail**: 104,112 overview mismatches |

Relative to native in the same combined binary and run sequence, Terrio is about 3.8× faster on RGB and 4.9× on Float32, using 1.6× and 2.6× the native peak
RSS. Its Float32 file is 1.95× larger and semantically different. The first-repeat output of each
writer/fixture is validated after all timed runs (later repeats are not re-validated); validation
never contributes to export time.

Clean build and artifact size (one sample each):

| Artifact | Build s | Size | gzip -9 | Brotli 11 |
|---|---:|---:|---:|---:|
| Native export executable, baseline | 136.8 | 1.58 MiB stripped | — | — |
| Native export executable, with Terrio | 239.3 | 8.13 MiB stripped | — | — |
| Wasm export module, baseline | 104.1 | 1.26 MiB | 0.301 MiB | 0.221 MiB |
| Wasm export module, with Terrio | 194.8 | 5.21 MiB | 1.365 MiB | 1.092 MiB |

For these isolated consumers, Terrio adds 102.5 s native clean build, 90.6 s wasm clean
build, 6.55 MiB stripped native binary, and 1.064 MiB gzip / 0.871 MiB Brotli wasm.
Unstripped native sizes are 1.73 MiB and 8.92 MiB. Raw pre-bindgen wasm sizes are 1.72 MiB
and 5.73 MiB. Exact bytes, hashes, commands and source hashes are in
[`results/builds.json`](results/builds.json).

Wasm runtime smoke: all nine cases export from both modules and all three hostile requests reject.
The baseline module passes all independent checks. The Terrio module reproduces the same six
semantic failures as desktop: Float32/no-data, UInt16, palette, classes, grey+alpha and RGBA.
See [`results/wasm-baseline.json`](results/wasm-baseline.json) and
[`results/wasm-terrio.json`](results/wasm-terrio.json).

## Concrete upstream gaps

1. **Honour categorical COG resampling.** `RasterWriteOptions.overview_resampling` is parsed only
   for non-COG TIFFs; COG's `nearest_overviews` stays false. Nearest requests currently average
   indexes into invented classes. Define/select nearest sampling semantics explicitly; native
   uses the clipped full-source block center, whereas the private nearest downsampler takes
   top-left. See [options](https://github.com/opengeos/terrio/blob/ce3c4463f19864284d743720123c4d022784c497/crates/terrio/src/drivers/gtiff_write.rs#L421-L451).
2. **Alpha-weight colour overviews.** Tagging the final band as alpha is insufficient: average
   colour with alpha weights and average alpha independently. Transparent black must not darken
   valid colours. See [downsample](https://github.com/opengeos/terrio/blob/ce3c4463f19864284d743720123c4d022784c497/crates/terrio/src/drivers/gtiff_write.rs#L1124-L1161).
3. **Preserve valid-support weights through recursive overviews**, or offer a direct-full-source
   mode. Averaging previously averaged cells equally changes results when source valid counts
   differ. UInt16 rounding and Float32 quantization also need a documented equivalence contract.
4. **Expose a bounded-memory COG output API** for borrowed samples/tiles and a `Write` sink,
   on native and wasm. The current public row writer rejects COG, and public byte writing takes
   different native/wasm paths. Remove the discarded allocation in `from_data_native`; allow
   safe construction without coupling consumers to all public raster fields.
5. **Continue dependency feature splitting through wbraster/wbgeotiff.** JPEG XL/WebP remain
   unconditional; writer-only consumers still build vector/CRS/archive/HDF helper code and
   duplicate codec versions. Codec-specific features would make build costs match actual needs.
   [#154](https://github.com/opengeos/terrio/pull/154) explicitly identifies the upstream codec gap.

The feature work is useful, but these correctness and memory/API gaps block replacing the current
writer. Re-run this same corpus after upstream fixes, then measure the actual production app
integration if adoption is reconsidered. No upstream issue or modification was created by this
experiment.
