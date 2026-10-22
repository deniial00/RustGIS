#!/usr/bin/env python3
"""Measure the real RustGIS desktop, CLI and web applications with and without Terrio.

Builds an isolated snapshot of this checkout's HEAD with app-size/terrio-cog-app.patch applied (the
patch routes the engine's real COG export through Terrio, behind cargo features), so no production
source is touched. Variants:
  baseline   no Terrio dependency
  replace    COG export goes through Terrio instead of the native writer (the adoption scenario)
  alongside  the native writer stays and Terrio is selectable with "writer": "terrio"

Desktop: `cargo build --release --locked -p rustgis -p rustgis-cli` (release CI's command), then
packaging/package.sh on macOS (.app inside a .dmg, and the CLI .tar.gz).
Web: `trunk build --release` from apps/rustgis-web (the web-deploy workflow's command) plus
`cargo xtask web-pwa`, as deployed.
Every build uses a fresh target directory, four Cargo jobs and no incremental compilation.
"""
import argparse
import gzip
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
FEATURES = {"baseline": [], "replace": ["terrio-cog"], "alongside": ["terrio-cog-alongside"]}


def run(cmd, cwd, env, log):
    start = time.perf_counter()
    with open(log, "w") as f:
        p = subprocess.run([str(c) for c in cmd], cwd=cwd, env=env, stdout=f, stderr=subprocess.STDOUT, text=True)
    elapsed = time.perf_counter() - start
    if p.returncode:
        raise SystemExit(f"FAILED ({p.returncode}): {' '.join(map(str, cmd))}\nsee {log}")
    return elapsed


def out(cmd, cwd=None):
    return subprocess.run([str(c) for c in cmd], cwd=cwd, check=True, capture_output=True, text=True).stdout


def dir_bytes(path):
    return sum(p.stat().st_size for p in Path(path).rglob("*") if p.is_file())


def brotli_bytes(path):
    return len(subprocess.run(["brotli", "-q", "11", "-c", str(path)], check=True, capture_output=True).stdout)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", required=True, type=Path, help="fresh directory for the snapshot, artifacts and results.json")
    parser.add_argument("--variants", nargs="+", choices=list(FEATURES), default=list(FEATURES))
    parser.add_argument("--targets", nargs="+", choices=["desktop", "web"], default=["desktop", "web"])
    parser.add_argument("--trunk", default="trunk", help="Trunk 0.21.14, as the web-deploy workflow pins")
    parser.add_argument("--resume", action="store_true", help="continue an interrupted --out: skip builds already in results.json")
    args = parser.parse_args()
    root = args.out.resolve()
    src = root / "src"
    report = root / "results.json"
    resumed = args.resume and report.exists()
    if root.exists() and not resumed:
        parser.error(f"{root} exists; select a fresh directory (or --resume an interrupted run)")
    if not resumed:
        (root / "logs").mkdir(parents=True)
        src.mkdir()
        subprocess.run(f"git archive HEAD | tar -x -C {src}", shell=True, check=True, cwd=REPO)
        shutil.rmtree(src / "experiments", ignore_errors=True)
        subprocess.run(["git", "apply", "--whitespace=nowarn", str(HERE / "app-size/terrio-cog-app.patch")], cwd=src, check=True)
    for leftover in list((root / "target").glob("*")) + list(root.glob("pkg-*")):
        shutil.rmtree(leftover, ignore_errors=True)

    env = dict(os.environ, CARGO_BUILD_JOBS="4", CARGO_INCREMENTAL="0")
    # Trunk's CLI rejects NO_COLOR=1 (it accepts only true/false); CI does not set it.
    env.pop("NO_COLOR", None)
    if platform.system() == "Darwin":
        # Apple's clang cannot target wasm; the C dependencies (zstd) need LLVM's.
        env.setdefault("CC_wasm32_unknown_unknown", "/opt/homebrew/opt/llvm/bin/clang")
        env.setdefault("AR_wasm32_unknown_unknown", "/opt/homebrew/opt/llvm/bin/llvm-ar")
        env.setdefault("CFLAGS_wasm32_unknown_unknown", "-D ZSTD_DISABLE_ASM=1")
    run(["cargo", "fetch", "--locked"], src, env, root / "logs/fetch.log")
    results = json.loads(report.read_text()) if resumed else {
        "base_commit": out(["git", "rev-parse", "HEAD"], REPO).strip(),
        "system": platform.platform(),
        "rustc": out(["rustc", "-Vv"]),
        "trunk": out([args.trunk, "--version"]).strip(),
        "method": "fresh target dir per build, dependencies pre-fetched, four Cargo jobs, no incremental, sequential builds, warm OS/Cargo caches",
        "desktop": {}, "web": {},
    }

    def save():
        report.write_text(json.dumps(results, indent=2) + "\n")

    for target in args.targets:
        for variant in args.variants:
            if variant in results[target]:
                continue
            features = FEATURES[variant]
            tdir = root / "target" / f"{target}-{variant}"
            e = dict(env, CARGO_TARGET_DIR=str(tdir))
            art = root / "artifacts" / variant
            art.mkdir(parents=True, exist_ok=True)
            row = {"features": features}
            if target == "desktop":
                cmd = ["cargo", "build", "--release", "--locked", "-p", "rustgis", "-p", "rustgis-cli"]
                if features:
                    cmd += ["--features", features[0]]
                row["command"] = cmd
                row["build_seconds"] = run(cmd, src, e, root / f"logs/desktop-{variant}.log")
                for name in ("rustgis", "rustgis-cli"):
                    built = tdir / "release" / name
                    kept = art / name
                    shutil.copy2(built, kept)
                    stripped = art / f"{name}.stripped"
                    shutil.copy2(built, stripped)
                    subprocess.run(["strip", "-x", str(stripped)], check=True)
                    row[name] = {"bytes": kept.stat().st_size, "stripped_bytes": stripped.stat().st_size}
                if platform.system() == "Darwin":
                    # packaging/package.sh reads target/release and writes dist/ beside it.
                    pkg = root / f"pkg-{variant}"
                    (pkg / "target").mkdir(parents=True)
                    shutil.copytree(src / "packaging", pkg / "packaging")
                    for f in ("LICENSE", "README.md"):
                        shutil.copy2(src / f, pkg / f)
                    (pkg / "target").joinpath("release").mkdir()
                    for name in ("rustgis", "rustgis-cli"):
                        shutil.copy2(tdir / "release" / name, pkg / "target/release" / name)
                    # The build tree is no longer needed; free its disk before the packaging tools run.
                    shutil.rmtree(tdir, ignore_errors=True)
                    # hdiutil intermittently reports "No space left on device" with ample free space
                    # (seen once or twice per run); retry the unchanged script and count the tries.
                    for attempt in range(1, 4):
                        shutil.rmtree(pkg / "dist", ignore_errors=True)
                        try:
                            run(["bash", pkg / "packaging/package.sh", "0.3.0"], pkg, env, root / f"logs/package-{variant}-attempt{attempt}.log")
                            break
                        except SystemExit:
                            if attempt == 3:
                                raise
                    row["package_attempts"] = attempt
                    dmg = next((pkg / "dist").glob("*.dmg"))
                    tgz = next((pkg / "dist").glob("*.tar.gz"))
                    mount = Path(tempfile.mkdtemp(prefix="rustgis-dmg-"))
                    subprocess.run(["hdiutil", "attach", "-nobrowse", "-readonly", "-mountpoint", str(mount), str(dmg)], check=True, capture_output=True)
                    try:
                        app_bytes = dir_bytes(mount / "RustGIS.app")
                    finally:
                        subprocess.run(["hdiutil", "detach", str(mount)], check=True, capture_output=True)
                    row["macos_package"] = {"app_bundle_bytes": app_bytes, "dmg_bytes": dmg.stat().st_size, "cli_tar_gz_bytes": tgz.stat().st_size}
                    shutil.copy2(dmg, art / dmg.name)
                    shutil.rmtree(pkg)
                results["desktop"][variant] = row
            else:
                dist = art / "web-dist"
                trunk = [args.trunk, "build", "--release", "--public-url", "/", "--dist", str(dist)]
                if features:
                    trunk += ["--features", features[0]]
                row["command"] = trunk
                row["build_seconds"] = run(trunk, src / "apps/rustgis-web", dict(e, PATH=f"{Path(args.trunk).resolve().parent}:{e['PATH']}"), root / f"logs/web-{variant}.log")
                run(["cargo", "run", "-q", "-p", "xtask", "--", "web-pwa", dist], src, e, root / f"logs/web-pwa-{variant}.log")
                files = {}
                for p in sorted(dist.iterdir()):
                    if not p.is_file():
                        continue
                    data = p.read_bytes()
                    entry = {"bytes": len(data), "gzip9_bytes": len(gzip.compress(data, 9, mtime=0))}
                    if p.suffix == ".wasm":
                        entry["brotli11_bytes"] = brotli_bytes(p)
                    files[p.name] = entry
                wasm = next(v for k, v in files.items() if k.endswith(".wasm"))
                row["files"] = files
                row["wasm"] = wasm
                row["dist_total_bytes"] = sum(v["bytes"] for v in files.values())
                row["dist_total_gzip9_bytes"] = sum(v["gzip9_bytes"] for v in files.values())
                results["web"][variant] = row
            shutil.rmtree(tdir, ignore_errors=True)
            save()
            print(json.dumps({target: variant, "build_seconds": row["build_seconds"]}), flush=True)
    save()
    print(f"results: {report}")


if __name__ == "__main__":
    sys.exit(main())
