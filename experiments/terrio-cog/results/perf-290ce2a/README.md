# Rerun: `perf-cog-average-rows`

**The performance branch is faster in a same-run comparison, without changing the checked pixels.**
It retains all nine original correctness passes on desktop and wasm. The known categorical
partial-block edge sampling difference remains unchanged.

## Revision and scope

- Repository/branch: [deniial00/terrio:perf-cog-average-rows](https://github.com/deniial00/terrio/tree/perf-cog-average-rows).
- Exact pin: [`290ce2aa997f40369e32293cc87475bee6444fe1`](https://github.com/deniial00/terrio/commit/290ce2aa997f40369e32293cc87475bee6444fe1), Terrio 0.4.0.
- Performance commit's parent: `fb0dd2ae537d374dec18c2e6ef107196436436fd` (0.4.0 release).
- Comparison executable: the retained, hash-verified exporter at `21bac2c0d645e5f9e12c345510e737ce706cc628` (Terrio 0.3.2).
- Only the Terrio dependency pins/lock entries changed for the adapter. Its Rust executable code,
  fixture definitions, CLI, independent pixel oracle, tile size, compression and other writer settings
  are unchanged. A new `compare_revisions.py` orchestrates paired exporter measurements.
- All non-Terrio package versions were retained. The original and previous fix-branch reports are
  unchanged; their preservation hashes are in [provenance.json](provenance.json).
- Production RustGIS writer/I/O and production manifests/lockfile remain untouched. No push,
  commit, issue edit or issue comment was made for this rerun.

The new code accumulates each source block in local sums/weights and parallelizes complete output
rows. See [average_block](https://github.com/deniial00/terrio/blob/290ce2aa997f40369e32293cc87475bee6444fe1/crates/terrio/src/drivers/gtiff_write.rs#L1132-L1172)
and [row averaging](https://github.com/deniial00/terrio/blob/290ce2aa997f40369e32293cc87475bee6444fe1/crates/terrio/src/drivers/gtiff_write.rs#L1268-L1318).

**Attribution limit:** the paired comparison is `21bac2c0` versus `290ce2aa`, not the performance
commit versus its immediate parent. The newer release base includes other changes, including
parallel overview-level construction. These results establish a whole-branch improvement, not a
profiled attribution of every saved millisecond to the row-averaging change. Earlier statements
that assigned a cause to old/new timing differences without profiling should not be quoted as proof.

## Same-run paired comparison

4096×4096, five fresh processes per branch/input. Both execute the Terrio path. Odd repetitions
run before/after; even repetitions reverse the order. No warmup discarded. No concurrent builds
or exports. Export timing includes conversion, compression, file output, sync and metadata,
not fixture construction. RSS is process peak, including fixtures and writer allocations.

| Fixture | Previous fix `21bac2c0` ms | Performance `290ce2aa` ms | Ratio of median times |
|---|---:|---:|---:|
| RGB UInt8 | 809.1 | 264.0 | 3.06× faster |
| Float32 + no-data | 264.9 | 98.0 | 2.70× faster |

First-repeat outputs were verified after all timing runs with the unchanged independent pixel
oracle and GDAL full COG validator. Those RGB and Float32 files are **byte-identical** between
branches (SHA-256 recorded in [paired.json](paired.json)). Later repeat outputs are not independently
pixel-validated or hashed; each run's process/protocol/size/RSS checks still pass.

Peak memory did not materially improve: paired medians are 319.3 → 320.6 MiB for RGB and
193.1 → 193.6 MiB for Float32. File sizes are unchanged between paired branches: 21,646,629 bytes
RGB and 3,327,228 bytes Float32.

This is a small synthetic benchmark on the same M1 Pro/16 GiB machine and rustc 1.98.1 as before,
not a representative corpus, a profile, or a confidence interval. Binary hashes and all 20 raw
process records are retained in [paired.json](paired.json).

## Correctness: no regression in the exercised paths

| Case | Native | Performance branch desktop | Performance branch wasm |
|---|---|---|---|
| RGB 1024×768 | Pass | Pass | Pass |
| Float32 + no-data | Pass | Pass | Pass |
| UInt16 + no-data | Pass | Pass | Pass |
| Palette | Pass | Pass | Pass |
| Classes | Pass | Pass | Pass |
| Grey + alpha | Pass | Pass | Pass |
| RGBA / alpha weighting | Pass | Pass | Pass |
| Odd RGB dimensions | Pass | Pass | Pass |
| Forced small BigTIFF | Pass | Pass | Pass |

Other original correctness cases use 1025×769, as before. All 18 desktop comparisons pass,
including all full-resolution and overview pixels, EPSG/geotransform, no-data, palette, storage,
alpha interpretation, tile, compression, predictor and TIFF-flavour checks. GDAL full COG
validation has no errors or warnings. The first-repeat 4096×4096 benchmark outputs also pass.

Both minimal native/wasm probe builds succeed. Actual wasm modules were bound with wasm-bindgen
0.2.129 and executed in Node. All nine native-only exports and all 18 combined-module exports
pass the same independent checks; the three hostile fixture requests are rejected in both modules.
This does not measure a browser UI, browser memory, or the internal streaming-slab API.

### The known 1027×771 edge difference remains

| Supplemental case | Native | Performance branch desktop | Performance branch wasm |
|---|---|---|---|
| Classes | Pass | 449 pixel differences | Same 449 differences |
| Palette | Pass | 449 pixel differences | Same 449 differences |
| Float32 + no-data | Pass | Pass | Pass |
| UInt16 + no-data | Pass | Pass | Pass |
| RGBA | Pass | Pass | Pass |

All supplemental files are valid COGs. Only the second categorical overview's bottom row/right
column differs: native takes the centre of the clipped block, while Terrio clamps the nominal
block centre to the image boundary. Class membership remains valid; this is not invented indexes
or an averaging regression. See [edges.json](edges.json) and [wasm-edges.json](wasm-edges.json).

This test uses the native writer's contract as its oracle. It is **not** a claim that GDAL mandates
native's nearest sampling convention. The direct GDAL coordinate-ramp evidence and correction to
the earlier centre-pixel claim remain in the [previous report](../fork-21bac2c/README.md#supplemental-1027771-clipped-edges).
Neither posted issue was changed during this rerun.

## Comparison against native

Separate standard comparison: same combined executable, median of three fresh processes, with
native then Terrio per repetition. First-repeat output verification follows all timing runs.

| Fixture | Writer | Export ms | Peak RSS MiB | File MiB |
|---|---|---:|---:|---:|
| RGB | Native | 1183.0 | 202.2 | 21.88 |
| RGB | Performance Terrio | 262.1 | 319.2 | 20.64 |
| Float32 + no-data | Native | 405.3 | 74.1 | 1.55 |
| Float32 + no-data | Performance Terrio | 93.7 | 192.0 | 3.17 |

About 4.5× and 4.3× faster than native on these inputs, but about 1.6× and 2.6× its peak memory.
Float32 output remains about 2.05× larger. Deflate implementations differ (native miniz_oxide
versus Terrio's resolved flate2/zlib-rs), and native is serial while Terrio uses default parallelism.
Native-only baseline timings were measured separately: 1200.5 ms RGB and 419.7 ms Float32;
[baseline-performance.json](baseline-performance.json) contains the raw runs and independent checks.

## Repeated clean build and size costs

| Consumer | Clean build s | Native stripped MiB | Post-bindgen wasm MiB | gzip-9 MiB | Brotli-11 MiB |
|---|---:|---:|---:|---:|---:|
| Native, without Terrio | 138.5 | 1.583 | — | — | — |
| Native, performance branch linked | 237.9 | 8.256 | — | — | — |
| Wasm, without Terrio | 86.2 | — | 1.259 | 0.301 | 0.221 |
| Wasm, performance branch linked | 136.6 | — | 5.309 | 1.391 | 1.110 |

One fresh target directory per variant, four Cargo jobs, incremental off, dependencies fetched
before timing, warm OS/source caches, sequential builds. Native uses thin LTO/16 codegen units;
wasm uses fat LTO/one codegen unit. No wasm-opt. Native size uses `strip -x`; compressed wasm
is measured after wasm-bindgen. These are linked export harnesses, not full RustGIS applications.
Single build-time samples are not causal speedup claims across runs.

Relative to the previous fix branch, the native consumer grows 130,912 bytes and gzip wasm grows
5,074 bytes. Exact bytes, commands and matching source hashes are in [builds.json](builds.json).
The graph still contains 118 native and 107 wasm package/version identities. JPEG XL, WebP,
vector/CRS/archive helpers and wbhdf remain. The adapter's full selected-image typed copy and
Terrio's retained overview/chunk storage are not removed by this performance change.

## Reproduce

Use the current exact pin in both manifests and a fresh output path. Prerequisites remain those
of the top-level README. From `experiments/terrio-cog`:

```sh
cargo build --locked --manifest-path probe/Cargo.toml
cargo build --locked --manifest-path probe/Cargo.toml --target wasm32-unknown-unknown
python3 measure_build.py native-terrio --out .local/repro-perf
python3 compare.py --binary .local/repro-perf/native-terrio --out .local/repro-perf/desktop --repeats 3 --size 4096
python3 measure_build.py native-baseline --out .local/repro-perf
python3 measure_build.py wasm-baseline --out .local/repro-perf
python3 measure_build.py wasm-terrio --out .local/repro-perf
node smoke_wasm.cjs .local/repro-perf/wasm-baseline/eval.js .local/repro-perf/wasm-baseline-exports native
node smoke_wasm.cjs .local/repro-perf/wasm-terrio/eval.js .local/repro-perf/wasm-terrio-exports both
python3 verify_wasm.py .local/repro-perf/wasm-baseline-exports
python3 verify_wasm.py .local/repro-perf/wasm-terrio-exports
python3 compare.py --binary .local/repro-perf/native-baseline --writers native --phase performance --out .local/repro-perf/baseline-performance --repeats 3 --size 4096
```

For the paired comparison, retain the previous executable as this evaluation did, or first build
the same harness with both manifests pinned to `21bac2c0d645e5f9e12c345510e737ce706cc628` and run
`cargo update -p terrio` for both lockfiles. Save its `native-terrio` artifact in a separate directory.
Restore the `290ce2aa` manifests/lockfiles before building the new exporter. Do not rebuild both
into the same saved artifact path. Then run:

```sh
python3 compare_revisions.py \
  --before .local/fork-21bac2c/native-terrio \
  --after .local/repro-perf/native-terrio \
  --out .local/repro-perf/paired --repeats 5 --size 4096
```

Supplemental native/wasm edge reproduction is identical to the previous report: use the unchanged
fixtures at 1027×771 and the unchanged `verify_file` oracle. Files generated by this run remain under
`.local/perf-290ce2a/`; JSON evidence is retained here. The original seven result files and the prior
fix-branch report/evidence were hash-checked unchanged.

## Decision

The measured branch-level performance gain is substantial and the original corpus remains correct.
Keep this as an integration candidate, not an automatic production cutover: the known nearest-edge
policy difference, full-image conversion/retention costs, unconditional codec graph and actual
production application size/build costs still need an adoption decision. Coverage limits from the
original evaluation remain: no >4 GiB threshold exercise, broader real-data corpus, browser UI,
Windows/Linux execution, or arbitrary CRS/type/transparent-palette claim.
