#!/usr/bin/env python3
"""Measure clean-target linked export consumers, not the full RustGIS GUI applications."""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import time

ROOT = Path(__file__).resolve().parent


def run(cmd, env=None):
    p = subprocess.run([str(x) for x in cmd], cwd=ROOT, env=env, text=True, capture_output=True)
    if p.returncode:
        raise RuntimeError(f"{cmd}:\n{p.stdout}\n{p.stderr}")
    return p.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("variant", choices=["native-baseline", "native-terrio", "wasm-baseline", "wasm-terrio"])
    parser.add_argument("--out", type=Path, default=ROOT / ".local")
    parser.add_argument("--wasm-bindgen", default="wasm-bindgen")
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    target = out / ("build-" + args.variant)
    if target.exists():
        raise SystemExit(f"clean-target measurement requires new directory: {target}")
    wasm = args.variant.startswith("wasm")
    terrio = args.variant.endswith("terrio")
    env = dict(os.environ, CARGO_BUILD_JOBS="4", CARGO_INCREMENTAL="0")
    # LLVM clang supports wasm C dependencies; Apple clang lacks the wasm backend.
    if wasm and platform.system() == "Darwin":
        env.setdefault("CC_wasm32_unknown_unknown", "/opt/homebrew/opt/llvm/bin/clang")
        env.setdefault("AR_wasm32_unknown_unknown", "/opt/homebrew/opt/llvm/bin/llvm-ar")
        env.setdefault("CFLAGS_wasm32_unknown_unknown", "-D ZSTD_DISABLE_ASM=1")
    run(["cargo", "fetch", "--locked"], env)
    files = [ROOT / "Cargo.toml", ROOT / "Cargo.lock", *sorted((ROOT / "src").glob("*.rs"))]
    source_hashes = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in files}
    cmd = ["cargo", "build", "--locked", "--target-dir", target]
    if wasm:
        cmd += ["--profile", "web", "--target", "wasm32-unknown-unknown", "--lib"]
    else:
        cmd += ["--release", "--bin", "rustgis-terrio-cog-eval"]
    if terrio:
        cmd += ["--features", "terrio"]
    start = time.perf_counter()
    result = subprocess.run([str(x) for x in cmd], cwd=ROOT, env=env, text=True, capture_output=True)
    elapsed = time.perf_counter() - start
    (out / (args.variant + ".build.log")).write_text(result.stdout + result.stderr)
    if result.returncode:
        raise SystemExit(result.stderr)
    if any(hashlib.sha256((ROOT / name).read_bytes()).hexdigest() != digest for name, digest in source_hashes.items()):
        raise SystemExit("sources changed during measurement; repeat with a fresh target directory")
    row = {"variant": args.variant, "build_seconds": elapsed, "command": [str(x) for x in cmd], "jobs": 4,
           "incremental": False, "source_sha256": source_hashes, "compiler": run(["rustc", "-Vv"]), "system": platform.platform(),
           "method": "one fresh target directory; pre-fetched dependencies; warm OS/Cargo source caches; sequential builds; no app/UI linkage"}
    if wasm:
        source = target / "wasm32-unknown-unknown/web/rustgis_terrio_cog_eval.wasm"
        package = out / args.variant
        run([args.wasm_bindgen, source, "--target", "nodejs", "--out-dir", package, "--out-name", "eval"])
        linked = package / "eval_bg.wasm"
        data = linked.read_bytes()
        zipped = gzip.compress(data, compresslevel=9, mtime=0)
        brotli = subprocess.run(["brotli", "-q", "11", "-c", str(linked)], check=True, capture_output=True).stdout
        linked.with_suffix(".wasm.gz").write_bytes(zipped)
        linked.with_suffix(".wasm.br").write_bytes(brotli)
        row.update(raw_wasm_bytes=source.stat().st_size, bindgen_wasm_bytes=len(data), gzip9_bytes=len(zipped),
                   brotli11_bytes=len(brotli), wasm_bindgen=run([args.wasm_bindgen, "--version"]).strip(),
                   bindgen_js_bytes=(package / "eval.js").stat().st_size, artifact=str(linked))
    else:
        source = target / "release/rustgis-terrio-cog-eval"
        dest = out / args.variant
        shutil.copy2(source, dest)
        run(["strip", "-x", dest])
        row.update(binary_bytes=source.stat().st_size, stripped_binary_bytes=dest.stat().st_size, artifact=str(dest), strip="strip -x")
    row["sha256"] = hashlib.sha256(Path(row["artifact"]).read_bytes()).hexdigest()
    (out / (args.variant + ".json")).write_text(json.dumps(row, indent=2) + "\n")
    print(json.dumps(row, indent=2))


if __name__ == "__main__":
    main()
