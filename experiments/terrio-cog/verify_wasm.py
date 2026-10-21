#!/usr/bin/env python3
"""Apply the same independent GDAL checks to actual wasm smoke exports."""
import argparse
import json
from pathlib import Path

from osgeo import gdal
from compare import verify_file, save_report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    directory = args.directory.resolve()
    target = directory / "verification.json"
    if target.exists():
        parser.error("verification.json already exists; use a fresh smoke run")
    gdal.UseExceptions()
    smoke = json.loads((directory / "wasm-smoke.json").read_text())
    rows = []
    for row in smoke:
        if "hostile" in row:
            rows.append({**row, "status": "PASS" if row["rejected"] else "FAIL"})
        elif row["status"] == "exported":
            result = verify_file(Path(row["file"]), row["case"], row["width"], row["height"], 10)
            rows.append({"writer": row["writer"], "case": row["case"], **result})
        else:
            rows.append({**row, "status": "FAIL"})
    report = {"gdal": gdal.VersionInfo("--version"), "results": rows,
              "status": "FAIL" if any(r["status"] == "FAIL" for r in rows) else "PASS"}
    save_report(target, report)
    print(json.dumps({"report": str(target), "status": report["status"],
                      "pass": sum(r["status"] == "PASS" for r in rows),
                      "fail": sum(r["status"] == "FAIL" for r in rows)}))
    return int(report["status"] == "FAIL")


if __name__ == "__main__":
    raise SystemExit(main())
