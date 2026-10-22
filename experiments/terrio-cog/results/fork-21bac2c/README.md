# Rerun: Terrio COG overview fixes

**The fork fixes all six failures in the original evaluation corpus on desktop and wasm.**
One supplemental categorical edge case still differs from RustGIS's native sampling contract.
This is a sampling-policy difference, not invented class values or an invalid COG.

## Revision and unchanged experiment

- Fork: [deniial00/terrio](https://github.com/deniial00/terrio).
- Branch: [cog-overviews-honour-overview_resampling-weight](https://github.com/deniial00/terrio/tree/cog-overviews-honour-overview_resampling-weight).
- Exact revision: [`21bac2c0d645e5f9e12c345510e737ce706cc628`](https://github.com/deniial00/terrio/commit/21bac2c0d645e5f9e12c345510e737ce706cc628).
- Dependency remains `default-features = false, features = ["geotiff"]`.
- Both evaluation manifests/lockfiles now use this fork/revision. The adapter's executable code,
  fixtures, CLI, GDAL checks, exact-sample oracle, dimensions, and benchmark settings are unchanged.
  Only a stale adapter comment was updated alongside the dependency pin.
- All seven original result files in the parent directory remain byte-for-byte unchanged.
  The original `ce3c4463` source pin is reproducible from RustGIS commit `ee149b8`.
- No production RustGIS code, production manifests, or production lockfile changed. No issue
  comments, commits, or pushes were made for this rerun.

The branch also contains newer CRS/GeoKeys changes relative to `ce3c4463`. Build-size deltas
therefore describe the complete pinned branch, not an isolated causal measurement of the COG fix.
Only the Terrio git package changed in either lockfile; the other dependency versions were retained.

The fix parses COG `overview_resampling`, builds each overview directly from the full-resolution
samples, and weights colour averages by alpha. Source:
[options](https://github.com/deniial00/terrio/blob/21bac2c0d645e5f9e12c345510e737ce706cc628/crates/terrio/src/drivers/gtiff_write.rs#L418-L450),
[overview levels](https://github.com/deniial00/terrio/blob/21bac2c0d645e5f9e12c345510e737ce706cc628/crates/terrio/src/drivers/gtiff_write.rs#L704-L719),
[reducer](https://github.com/deniial00/terrio/blob/21bac2c0d645e5f9e12c345510e737ce706cc628/crates/terrio/src/drivers/gtiff_write.rs#L1213-L1326).

## Original corpus: fixed

| Case | Original Terrio `ce3c4463` | Fork `21bac2c0`, desktop | Fork `21bac2c0`, wasm |
|---|---|---|---|
| RGB 1024×768 | Pass | Pass | Pass |
| Float32 + no-data | Fail | Pass | Pass |
| UInt16 + no-data | Fail | Pass | Pass |
| Opaque palette | Fail | Pass | Pass |
| Classes | Fail | Pass | Pass |
| Grey + alpha | Fail | Pass | Pass |
| RGBA / alpha-weighted colour | Fail | Pass | Pass |
| Odd RGB dimensions | Pass | Pass | Pass |
| Forced BigTIFF | Pass | Pass | Pass |

Non-RGB correctness cases use 1025×769, as before. Both writers now pass all 18 desktop
comparisons, including every full-resolution and overview sample, no-data/validity, UInt16
storage, alpha interpretation, palette, EPSG:32617, exact geotransform, TIFF flavour, tile,
compression and predictor checks. Full GDAL COG validation reports no errors or warnings.
The larger 4096×4096 RGB and Float32/no-data benchmark outputs also pass their independent
pixel/overview checks (first-repeat outputs checked after all timings).

Both minimal native/wasm probe builds succeed. Both linked wasm modules export all original
cases through Node, with the same independent GDAL checks passing. All three hostile fixture
requests are rejected. Wasm/browser UI and browser peak memory are not measured.

Evidence: [desktop.json](desktop.json), [wasm-baseline.json](wasm-baseline.json),
[wasm-terrio.json](wasm-terrio.json), and [dependencies.json](dependencies.json).

## Supplemental 1027×771 clipped edges

The original 1025×769 dimensions leave one-pixel final blocks at overview factor 4. Adding
1027×771 exercises three-pixel final blocks. The original oracle was reused without modification.

| Supplemental case | Native | Fork desktop | Fork wasm |
|---|---|---|---|
| Classes | Pass | 449 sample mismatches | Same 449 mismatches |
| Palette | Pass | 449 sample mismatches | Same 449 mismatches |
| Float32 + no-data | Pass | Pass | Pass |
| UInt16 + no-data | Pass | Pass | Pass |
| RGBA | Pass | Pass | Pass |

All supplemental files pass GDAL full COG validation. Full-resolution samples and class membership
remain correct; no categories are invented. Only the second categorical overview (257×193) differs,
along the rightmost column and bottom row: `193 + 257 - 1 = 449` pixels.

The fork chooses `(block_start + factor/2).min(image_end)`; native chooses
`block_start + clipped_block_length/2`. At the right edge of a factor-4 overview of width 1027,
these select full-resolution x=1026 versus x=1025. At overview `(256,0)`, class values are 19
versus 7. The y edge has the same distinction. This is a remaining exact-native-equivalence gap;
which convention to require is a policy decision, not a categorical-membership failure.

**Correction to the earlier issue discussion:** the assertion that GDAL `NEAREST` necessarily
matches native's clipped-block-centre rule was not supported. A direct GDAL 3.13.3 MEM
`BuildOverviews("NEAREST", [2,4])` check on the same dimensions does not match either writer's
sampling grid. A UInt32 coordinate ramp (`value = y*1027+x`) confirms that GDAL selects source
`(0,0)` for the first pixel of both overview levels, while native and the fork select centres.
The exact selected positions and comparison counts are recorded in [edges.json](edges.json).
Do not report this edge mismatch as established GDAL noncompliance. The posted issues have
not been edited by this rerun.

Evidence: [edges.json](edges.json), [wasm-edges.json](wasm-edges.json).

## Repeated desktop costs

4096×4096; median of three fresh processes; native and Terrio paths in the same combined executable,
alternated native then Terrio. File creation, conversion, compression, final sync and metadata are
included; fixture construction is excluded. RSS includes fixture and all writer allocations.
These are synthetic inputs, not a representative corpus or confidence interval.

| Fixture | Writer | Export ms | Peak RSS MiB | File MiB |
|---|---|---:|---:|---:|
| RGB | Native | 1195.2 | 202.1 | 21.88 |
| RGB | Fork Terrio | 846.6 | 322.3 | 20.64 |
| Float32 + no-data | Native | 418.5 | 74.3 | 1.55 |
| Float32 + no-data | Fork Terrio | 284.6 | 194.3 | 3.17 |

The corrected fork remains about 1.4–1.5× faster than native on these inputs, but uses about
1.6× the RGB peak memory and 2.6× the Float32 peak memory. Its Float32 output is now semantically
equivalent on the benchmark, but about 2.05× larger. DEFLATE backend differences remain:
miniz_oxide for native versus the resolved flate2/zlib-rs backend for Terrio. Terrio's default
parallelism is retained; native is serial.

The original Terrio timings were 326.5 ms RGB and 84.8 ms Float32. Correct full-source overview
reduction now takes 846.6 ms and 284.6 ms in this rerun. These are different runs; the old Float32
output failed semantic checks. Do not interpret its old speed as equivalent-work throughput.
The native-only executable was also remeasured separately in [baseline-performance.json](baseline-performance.json).

## Repeated clean builds and sizes

| Consumer | Clean build s | Native stripped MiB | Post-bindgen wasm MiB | gzip-9 MiB | Brotli-11 MiB |
|---|---:|---:|---:|---:|---:|
| Native, without Terrio | 131.7 | 1.583 | — | — | — |
| Native, fork Terrio linked | 238.1 | 8.131 | — | — | — |
| Wasm, without Terrio | 80.6 | — | 1.259 | 0.301 | 0.221 |
| Wasm, fork Terrio linked | 131.1 | — | 5.276 | 1.386 | 1.108 |

Same method as the original run: absent target directories, pre-fetched dependencies, warm source/OS
caches, four Cargo jobs, incremental off, sequential builds. Native release uses thin LTO/16 codegen
units; wasm web profile uses fat LTO/one codegen unit. Sizes are actual linked export harnesses,
not full RustGIS desktop/web application sizes. No wasm-opt. Build time is one sample per variant;
ambient load/caches/power are not controlled, so old/new build seconds are not causal speedup claims.
The stripped fork native executable is 2,448 bytes larger than the old linked consumer, and gzip wasm
is 22,188 bytes larger. Exact bytes, source hashes and artifact hashes: [builds.json](builds.json).

The minimal graph is still 118 native and 107 wasm package/version identities, with JPEG XL, WebP,
vector/CRS/archive helpers and wbhdf retained. The public owned-raster adapter still makes one full
selected-image typed conversion buffer. Terrio's memory COG preparation still retains full overview
arrays and compressed chunks, despite the reducer's bounded row accumulators. No public borrowed
COG output API or codec-feature improvement is claimed by this branch.

## Recommendation

The original six correctness blockers are resolved for the unchanged corpus, on both targets.
This fork is a substantially stronger candidate, but it is not an exact drop-in for native's
partial-block nearest convention. Keep the integration experimental until that sampling policy
is agreed, the scope of inputs is broadened, and actual production-app build/download costs are
measured if adoption is pursued. Memory/copy and dependency costs remain material.

Production writer and other I/O remain untouched. Unsupported adapter types/georeferencing/palette
transparency and the original evaluation's coverage limits are unchanged. No >4 GiB automatic
threshold test, real-data corpus, Windows/Linux build, or browser UI measurement is claimed.

## Reproduce this pin

From `experiments/terrio-cog`, with the prerequisites in the top-level README and a fresh output path:

```sh
cargo tree --locked --manifest-path probe/Cargo.toml -e normal --prefix none
cargo build --locked --manifest-path probe/Cargo.toml
cargo build --locked --manifest-path probe/Cargo.toml --target wasm32-unknown-unknown

python3 measure_build.py native-terrio --out .local/repro-21bac2c
python3 compare.py --binary .local/repro-21bac2c/native-terrio --out .local/repro-21bac2c/desktop --repeats 3 --size 4096
python3 measure_build.py native-baseline --out .local/repro-21bac2c
python3 measure_build.py wasm-baseline --out .local/repro-21bac2c
python3 measure_build.py wasm-terrio --out .local/repro-21bac2c

node smoke_wasm.cjs .local/repro-21bac2c/wasm-baseline/eval.js .local/repro-21bac2c/wasm-baseline-exports native
node smoke_wasm.cjs .local/repro-21bac2c/wasm-terrio/eval.js .local/repro-21bac2c/wasm-terrio-exports both
python3 verify_wasm.py .local/repro-21bac2c/wasm-baseline-exports
python3 verify_wasm.py .local/repro-21bac2c/wasm-terrio-exports
python3 compare.py --binary .local/repro-21bac2c/native-baseline --writers native --phase performance --out .local/repro-21bac2c/baseline-performance --repeats 3 --size 4096
```

All commands above pass at this pin. To reproduce the supplemental native edge checks without
changing any fixture/oracle (use an absent `edge-out` directory):

```python
from pathlib import Path
from compare import export, verify_file, save_report
binary = Path('.local/repro-21bac2c/native-terrio').resolve()
out = Path('edge-out').resolve()
out.mkdir()
rows = []
for case in ('classes', 'palette', 'float-nodata', 'uint16', 'rgba'):
    for writer in ('native', 'terrio'):
        path = out / f'{writer}-{case}.tif'
        row = export(binary, writer, case, 1027, 771, path, False, None)
        row['verification'] = verify_file(path, case, 1027, 771, 10)
        rows.append(row)
save_report(out / 'report.json', {'runs': rows})
```

`export_case(writer, case, 1027, 771, false)` on the generated wasm module exercises the same edge
inputs; [wasm-edges.json](wasm-edges.json) contains the observed native/fork results and TIFF paths.
