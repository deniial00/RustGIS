#!/usr/bin/env python3
"""Export COGs through the real rustgis-cli of each measured application variant and verify them.

Each input is a COG the native writer made from the evaluation's fixtures. The CLI opens it with
`--data` and runs the app's own `share.exportRaster` command with `"cog": true`, so the whole path
(reader, engine command, Terrio or native writer) is the shipped one. The output is then checked
with compare.py's independent oracle and GDAL's COG validator against the same fixture.
The `alongside` variant is exported twice: by default (native) and with "writer": "terrio".
The forced-BigTIFF case is skipped: the command only chooses the flavour itself.
"""
import argparse
import json
from pathlib import Path
import subprocess

from osgeo import gdal

from compare import save_report, verify_file

CASES = ("rgb", "float-nodata", "uint16", "palette", "classes", "gray-alpha", "rgba", "odd")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifacts", required=True, type=Path, help="measure_app_size.py's artifacts directory")
    parser.add_argument("--inputs", required=True, type=Path, help="a compare.py run's correctness directory")
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    if args.out.exists():
        parser.error("select a fresh output directory")
    args.out.mkdir(parents=True)
    gdal.UseExceptions()
    modes = [("baseline", None), ("replace", None), ("alongside", None), ("alongside", "terrio")]
    report = {"runs": []}
    for variant, writer in modes:
        cli = args.artifacts / variant / "rustgis-cli"
        for case in CASES:
            width, height = (1024, 768) if case == "rgb" else (1025, 769)
            source = args.inputs / f"native-{case}-{width}x{height}.tif"
            target = args.out / f"{variant}{'-' + writer if writer else ''}-{case}.tif"
            params = {"path": str(target), "cog": True, **({"writer": writer} if writer else {})}
            p = subprocess.run([str(cli), "run", "--no-plugins", "--data", str(source), "--cmd", f"share.exportRaster={json.dumps(params)}"],
                               capture_output=True, text=True)
            row = {"variant": variant, "writer": writer or "default", "case": case, "returncode": p.returncode, "stderr": p.stderr.strip()[-300:]}
            if p.returncode == 0 and target.is_file():
                v = verify_file(target, case, width, height, 10)
                row.update(status=v["status"], bytes=target.stat().st_size,
                           validator_errors=v["cog_validator"]["errors"], validator_warnings=v["cog_validator"]["warnings"],
                           failed_checks=[c["name"] for c in v["checks"] if c["status"] == "FAIL"])
            else:
                row["status"] = "FAIL"
            report["runs"].append(row)
            print(f"{row['variant']:9} {row['writer']:8} {case:13} {row['status']}  {row.get('failed_checks', '')}", flush=True)
    report["status"] = "FAIL" if any(r["status"] == "FAIL" for r in report["runs"]) else "PASS"
    save_report(args.out / "report.json", report)
    print(json.dumps({"status": report["status"], "report": str(args.out / 'report.json')}))
    return int(report["status"] == "FAIL")


if __name__ == "__main__":
    raise SystemExit(main())
