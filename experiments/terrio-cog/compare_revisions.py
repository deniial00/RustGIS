#!/usr/bin/env python3
"""Compare two real Terrio-linked exporters in alternating fresh processes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import statistics

from osgeo import gdal
from compare import export, positive_integer, save_report, verify_file


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", required=True, type=Path)
    parser.add_argument("--after", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--repeats", type=positive_integer, default=5)
    parser.add_argument("--size", type=positive_integer, default=4096)
    args = parser.parse_args()
    binaries = {"before": args.before.resolve(), "after": args.after.resolve()}
    for name, binary in binaries.items():
        if not binary.is_file() or not os.access(binary, os.X_OK):
            parser.error(f"{name} is not an executable file: {binary}")
    out = args.out.resolve()
    if out.exists():
        parser.error("select a fresh output directory")
    out.mkdir(parents=True)
    gdal.UseExceptions()
    report = {
        "configuration": {"size": args.size, "repeats": args.repeats},
        "binaries": {name: {"path": str(p), "sha256": hashlib.sha256(p.read_bytes()).hexdigest()} for name, p in binaries.items()},
        "method": "Fresh processes, Terrio file exports, five repeats by default; odd repeats before/after, even repeats after/before. Export timing and RSS use compare.py's unchanged contract. First-repeat outputs are verified after all timings. No warmup discarded.",
        "thread_environment": {name: os.environ.get(name) for name in ("RAYON_NUM_THREADS", "OMP_NUM_THREADS")},
        "runs": [], "summary": [], "verification": [],
    }
    target = out / "report.json"
    for case in ("rgb", "float-nodata"):
        for repeat in range(1, args.repeats + 1):
            order = ("before", "after") if repeat % 2 else ("after", "before")
            for variant in order:
                path = out / f"{variant}-{case}-{repeat}.tif"
                row = export(binaries[variant], "terrio", case, args.size, args.size, path, True, None)
                row.update(variant=variant, repeat=repeat, sequence=len(report["runs"]) + 1)
                report["runs"].append(row)
                save_report(target, report)
                print(f"{variant} {case} {repeat}: {row['status']}", flush=True)
    for case in ("rgb", "float-nodata"):
        for variant in binaries:
            rows = [r for r in report["runs"] if r["case"] == case and r["variant"] == variant]
            good = [r for r in rows if r["status"] == "PASS"]
            summary = {"case": case, "variant": variant, "successful_runs": len(good), "expected_runs": args.repeats}
            for key in ("export_ms", "process_wall_ms", "peak_rss_bytes", "bytes"):
                summary[key + "_median"] = statistics.median(r[key] for r in good) if good else None
            report["summary"].append(summary)
            first = next(r for r in rows if r["repeat"] == 1)
            path = Path(first["path"])
            check = verify_file(path, case, args.size, args.size, 10) if path.is_file() else {"status": "FAIL", "error": "missing output"}
            if path.is_file():
                check["sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
            report["verification"].append({"case": case, "variant": variant, **check})
    report["status"] = "FAIL" if any(r["status"] == "FAIL" for r in report["runs"] + report["verification"]) else "PASS"
    save_report(target, report)
    print(json.dumps({"status": report["status"], "report": str(target)}))
    return int(report["status"] == "FAIL")


if __name__ == "__main__":
    raise SystemExit(main())
