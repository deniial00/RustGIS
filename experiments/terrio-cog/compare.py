#!/usr/bin/env python3
"""Export fresh-process fixtures, verify COGs with GDAL, and retain JSON evidence.

No writer is the oracle: expected pixels are calculated here from fixture formulas.
A FAIL is evidence, including an unsupported upstream feature, not an exemption.
"""

import argparse
import contextlib
import datetime
import hashlib
import io
import json
import math
import os
from pathlib import Path
import platform
import re
import statistics
import struct
import subprocess
import sys
import time
import traceback

import numpy as np
from osgeo import gdal, osr
from osgeo_utils.samples import validate_cloud_optimized_geotiff as cog_validator


CASES = ("rgb", "float-nodata", "uint16", "palette", "classes", "gray-alpha", "rgba", "odd", "bigtiff")
WRITERS = ("native", "terrio")
GEOTRANSFORM = (500000.0, 30.0, 0.0, 4000000.0, 0.0, -30.0)
PALETTE = {1: (220, 30, 40, 255), 7: (20, 210, 50, 255), 19: (20, 40, 230, 255)}
TIFF_TAGS = {256, 257, 258, 259, 262, 277, 284, 317, 320, 322, 323, 338, 339, 42113}


def json_value(value):
    if isinstance(value, dict):
        return {str(k): json_value(v) for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [json_value(v) for v in value]
    if isinstance(value, np.ndarray):
        return json_value(value.tolist())
    if isinstance(value, np.generic):
        return json_value(value.item())
    if isinstance(value, float) and not math.isfinite(value):
        return str(value)
    return value


def check(checks, name, passed, **details):
    checks.append({"name": name, "status": "PASS" if passed else "FAIL", **json_value(details)})


def exception_record():
    return {"error": str(sys.exception()), "traceback": traceback.format_exc()}


def expected_fixture(case, width, height):
    y, x = np.indices((height, width), dtype=np.int64)
    holes = (x % 17 == 0) & (y % 19 == 0)
    if case in ("rgb", "odd", "bigtiff"):
        arrays = [x % 256, y % 256, np.where((x // 2 + y // 2) % 2 != 0, 100, 200)]
        return np.array(arrays, dtype=np.uint8), None, ["Red", "Green", "Blue"]
    if case == "float-nodata":
        a = ((x * 3 + y * 5) % 1000).astype(np.float32) / np.float32(4)
        a[holes] = -9999
        return a[np.newaxis], -9999.0, ["Gray"]
    if case == "uint16":
        a = ((x * 137 + y * 251) % 65535).astype(np.uint16)
        a[holes] = 65535
        return a[np.newaxis], 65535, ["Gray"]
    if case in ("classes", "palette"):
        a = np.array([1, 7, 19], dtype=np.uint8)[(x + y) % 3]
        return a[np.newaxis], None, ["Palette" if case == "palette" else "Gray"]
    even = x % 2 == 0
    if case == "gray-alpha":
        return np.array([np.where(even, 200, 0), np.where(even, 255, 0)], dtype=np.uint8), None, ["Gray", "Alpha"]
    if case == "rgba":
        return np.array([np.where(even, v, 0) for v in (200, 100, 50, 255)], dtype=np.uint8), None, ["Red", "Green", "Blue", "Alpha"]
    raise ValueError(f"unknown fixture {case}")


def expected_dimensions(width, height):
    dimensions = []
    factor = 2
    while max(width, height) > 512:
        width, height = (width + 1) // 2, (height + 1) // 2
        dimensions.append((width, height, factor))
        factor *= 2
    return dimensions


def block_sum(array, factor):
    """Sum clipped full-source blocks in float64, including odd right/bottom edges."""
    return np.add.reduceat(np.add.reduceat(array.astype(np.float64), np.arange(0, array.shape[0], factor), axis=0), np.arange(0, array.shape[1], factor), axis=1)


def expected_overview(source, case, nodata, factor):
    height, width = source.shape[1:]
    xs, ys = np.arange(0, width, factor), np.arange(0, height, factor)
    if case in ("classes", "palette"):
        cx = xs + np.minimum(factor, width - xs) // 2
        cy = ys + np.minimum(factor, height - ys) // 2
        return source[:, cy[:, None], cx[None, :]]
    alpha = source[-1] if case in ("rgba", "gray-alpha") else None
    result = []
    for index, band in enumerate(source):
        valid = np.ones(band.shape, dtype=bool) if nodata is None else band != nodata
        weights = valid.astype(np.float64)
        if alpha is not None and index != source.shape[0] - 1:
            weights *= alpha
        total_weight = block_sum(weights, factor)
        total = block_sum(band.astype(np.float64) * weights, factor)
        mean = np.full(total.shape, np.nan, dtype=np.float64)
        np.divide(total, total_weight, out=mean, where=total_weight > 0)
        # Rust converts the f64 mean to f32 before its integer f32.round().
        mean = mean.astype(np.float32)
        if source.dtype.kind in "ui":
            mean = np.floor(mean.astype(np.float64) + 0.5)
            mean = np.nan_to_num(mean, nan=float(nodata or 0)).astype(source.dtype)
        else:
            mean = np.where(np.isnan(mean), nodata if nodata is not None else np.nan, mean).astype(source.dtype)
        result.append(mean)
    return np.array(result)


def compare_arrays(actual, expected, max_examples):
    actual, expected = np.asarray(actual), np.asarray(expected)
    rows, cols = min(actual.shape[0], expected.shape[0]), min(actual.shape[1], expected.shape[1])
    a, e = actual[:rows, :cols], expected[:rows, :cols]
    same = (a == e) | (np.isnan(a) & np.isnan(e))
    bad = ~same
    locations = np.argwhere(bad)
    overlap_mismatches = int(np.count_nonzero(bad))
    missing = int(expected.size - rows * cols)
    excess = int(actual.size - rows * cols)
    examples = []
    for y, x in locations[:max_examples]:
        examples.append({"x": int(x), "y": int(y), "expected": json_value(e[y, x]), "actual": json_value(a[y, x])})
    delta = np.abs(a.astype(np.float64) - e.astype(np.float64))
    finite_delta = delta[np.isfinite(delta)]
    return {"expected_shape": list(expected.shape), "actual_shape": list(actual.shape),
            "compared_samples": rows * cols, "overlap_mismatches": overlap_mismatches,
            "missing_samples": missing, "excess_samples": excess,
            "mismatch_count": overlap_mismatches + missing + excess,
            "max_abs_error_finite": float(np.max(finite_delta)) if finite_delta.size else None,
            "examples": examples}


def tiff_directories(path):
    """Read only bounded TIFF directory tags; GDAL independently decodes the pixels."""
    size = path.stat().st_size
    with path.open("rb") as stream:
        def fetch(offset, length):
            if offset < 0 or length < 0 or offset + length > size:
                raise ValueError(f"TIFF range outside file: {offset}+{length}/{size}")
            stream.seek(offset)
            data = stream.read(length)
            if len(data) != length:
                raise ValueError("short TIFF read")
            return data

        header = fetch(0, 8)
        endian = "<" if header[:2] == b"II" else ">" if header[:2] == b"MM" else None
        if endian is None:
            raise ValueError("not a TIFF byte-order signature")
        magic = struct.unpack(endian + "H", header[2:4])[0]
        big = magic == 43
        if magic not in (42, 43):
            raise ValueError(f"invalid TIFF magic {magic}")
        if big and struct.unpack(endian + "HH", header[4:8]) != (8, 0):
            raise ValueError("invalid BigTIFF offset size/reserved bytes")
        offset = struct.unpack(endian + ("Q" if big else "I"), fetch(8, 8) if big else header[4:8])[0]
        inline, entry_size, count_size = (8, 20, 8) if big else (4, 12, 2)
        types = {1: ("B", 1), 2: ("s", 1), 3: ("H", 2), 4: ("I", 4), 5: (None, 8), 6: ("b", 1), 7: ("B", 1), 8: ("h", 2), 9: ("i", 4), 11: ("f", 4), 12: ("d", 8), 16: ("Q", 8), 17: ("q", 8), 18: ("Q", 8)}
        directories, seen = [], set()
        while offset:
            if offset in seen or len(directories) >= 64:
                raise ValueError("TIFF directory cycle or excessive directory count")
            seen.add(offset)
            count = struct.unpack(endian + ("Q" if big else "H"), fetch(offset, count_size))[0]
            if count > 65536:
                raise ValueError("excessive TIFF entry count")
            entries = fetch(offset + count_size, count * entry_size + inline)
            tags = {}
            for index in range(count):
                entry = entries[index * entry_size:(index + 1) * entry_size]
                tag, kind, n = struct.unpack(endian + ("HHQ" if big else "HHI"), entry[:12 if big else 8])
                if tag not in TIFF_TAGS:
                    continue
                fmt, width = types.get(kind, (None, 0))
                if width == 0 or n * width > 1048576:
                    raise ValueError(f"unsupported or excessive TIFF tag {tag} type {kind} count {n}")
                slot = entry[12:] if big else entry[8:]
                data = slot[:n * width] if n * width <= inline else fetch(struct.unpack(endian + ("Q" if big else "I"), slot)[0], n * width)
                if kind == 2:
                    value = data.rstrip(b"\x00").decode("ascii", errors="replace")
                elif fmt:
                    values = list(struct.unpack(endian + str(n) + fmt, data))
                    value = values[0] if n == 1 else values
                else:
                    value = data.hex()
                tags[str(tag)] = value
            directories.append({"offset": offset, "tags": tags})
            offset = struct.unpack(endian + ("Q" if big else "I"), entries[count * entry_size:])[0]
    return {"magic_hex": header[:4].hex(), "byte_order": "little" if endian == "<" else "big", "bigtiff": big, "directories": directories}


def band_metadata(band):
    table = band.GetColorTable()
    return {"width": band.XSize, "height": band.YSize, "dtype": gdal.GetDataTypeName(band.DataType),
            "color_interpretation": gdal.GetColorInterpretationName(band.GetColorInterpretation()),
            "nodata": json_value(band.GetNoDataValue()), "block_size": band.GetBlockSize(),
            "overview_count": band.GetOverviewCount(), "metadata": band.GetMetadata(),
            "image_structure": band.GetMetadata("IMAGE_STRUCTURE"), "tiff": band.GetMetadata("TIFF"),
            "palette": [table.GetColorEntry(i) for i in range(table.GetCount())] if table else None}


def verify_band(band, expected, case, nodata, interpretation, name, checks, max_examples, overview):
    metadata = band_metadata(band)
    expected_dtype = {np.dtype("uint8"): "Byte", np.dtype("uint16"): "UInt16", np.dtype("float32"): "Float32"}[expected.dtype]
    check(checks, name + ".dtype", metadata["dtype"] == expected_dtype, expected=expected_dtype, actual=metadata["dtype"])
    check(checks, name + ".color_interpretation", metadata["color_interpretation"] == interpretation, expected=interpretation, actual=metadata["color_interpretation"])
    check(checks, name + ".tile", metadata["block_size"] == [512, 512], expected=[512, 512], actual=metadata["block_size"])
    # TIFF stores nodata on the main IFD; GDAL may or may not inherit it on overviews.
    if not overview:
        check(checks, name + ".nodata", band.GetNoDataValue() == nodata, expected=nodata, actual=band.GetNoDataValue())
    if case == "palette":
        table = metadata["palette"]
        wanted = [list(PALETTE.get(i, (0, 0, 0, 255))) for i in range(256)]
        palette_errors = [] if table is None else [{"index": i, "expected": wanted[i], "actual": table[i]} for i in range(min(256, len(table))) if list(table[i]) != wanted[i]]
        check(checks, name + ".palette", table is not None and len(table) == 256 and not palette_errors,
              expected_entries=256, actual_entries=len(table) if table is not None else 0,
              mismatch_count=(256 if table is None else len(palette_errors) + abs(len(table) - 256)), examples=palette_errors[:max_examples])
    actual = band.ReadAsArray()
    if actual is None:
        raise ValueError(f"GDAL could not read {name}")
    comparison = compare_arrays(actual, expected, max_examples)
    pixel_check = ".categorical_full_source_center" if overview and case in ("classes", "palette") else (
        ".alpha_weighted_colour" if overview and case in ("gray-alpha", "rgba") and interpretation != "Alpha" else ".pixels")
    check(checks, name + pixel_check, comparison["mismatch_count"] == 0, **comparison)
    if case in ("classes", "palette"):
        values, counts = np.unique(actual, return_counts=True)
        invented = ~np.isin(actual, [1, 7, 19])
        locations = np.argwhere(invented)
        check(checks, name + ".categorical_membership", not np.any(invented),
              mismatch_count=int(np.count_nonzero(invented)), values=[{"value": json_value(v), "count": int(n)} for v, n in zip(values, counts)],
              examples=[{"x": int(x), "y": int(y), "actual": json_value(actual[y, x])} for y, x in locations[:max_examples]])
    if nodata is not None:
        check(checks, name + ".nodata_pixel_count", int(np.count_nonzero(actual == nodata)) == int(np.count_nonzero(expected == nodata)),
              actual=int(np.count_nonzero(actual == nodata)), expected=int(np.count_nonzero(expected == nodata)))
        expected_valid = np.isfinite(expected) & (expected != nodata)
        actual_valid = np.isfinite(actual) & (actual != nodata)
        validity = compare_arrays(actual_valid, expected_valid, max_examples)
        check(checks, name + ".validity_mask", validity["mismatch_count"] == 0, **validity)
    metadata["comparison"] = comparison
    return metadata


def verify_file(path, case, width, height, max_examples):
    checks, result = [], {"path": str(path), "checks": []}
    result["checks"] = checks
    source, nodata, interpretations = expected_fixture(case, width, height)
    dimensions = expected_dimensions(width, height)
    result["expected"] = {"dimensions": [width, height], "overview_dimensions": [[w, h] for w, h, _ in dimensions],
                          "overview_full_source_factors": [f for _, _, f in dimensions], "nodata": nodata,
                          "dtype": str(source.dtype), "color_interpretations": interpretations}
    try:
        result["bytes"] = path.stat().st_size
        tiff = tiff_directories(path)
        result["tiff"] = tiff
        check(checks, "tiff.bigtiff", tiff["bigtiff"] == (case == "bigtiff"), expected=(case == "bigtiff"), actual=tiff["bigtiff"], magic_hex=tiff["magic_hex"])
        check(checks, "tiff.directory_count", len(tiff["directories"]) == len(dimensions) + 1, expected=len(dimensions) + 1, actual=len(tiff["directories"]))
        for i, directory in enumerate(tiff["directories"]):
            tags = directory["tags"]
            check(checks, f"tiff.ifd[{i}].deflate", tags.get("259") in (8, 32946), expected=[8, 32946], actual=tags.get("259"))
            check(checks, f"tiff.ifd[{i}].predictor", tags.get("317", 1) == 1, expected=1, actual=tags.get("317", 1))
            check(checks, f"tiff.ifd[{i}].tile", [tags.get("322"), tags.get("323")] == [512, 512], expected=[512, 512], actual=[tags.get("322"), tags.get("323")])
    except Exception:
        check(checks, "tiff.inspect", False, **exception_record())
    validator_stdout, validator_stderr = io.StringIO(), io.StringIO()
    try:
        with contextlib.redirect_stdout(validator_stdout), contextlib.redirect_stderr(validator_stderr):
            warnings, errors, details = cog_validator.validate(str(path), full_check=True)
        result["cog_validator"] = {"full_check": True, "warnings": warnings, "errors": errors, "details": details}
        check(checks, "gdal.cog_validator", not errors, warnings=warnings, errors=errors)
    except Exception:
        result["cog_validator"] = {"full_check": True, "warnings": [], "errors": [str(sys.exception())], **exception_record()}
        check(checks, "gdal.cog_validator", False, **exception_record())
    result["cog_validator"].update(stdout=validator_stdout.getvalue(), stderr=validator_stderr.getvalue())
    try:
        dataset = gdal.Open(str(path), gdal.GA_ReadOnly)
        if dataset is None:
            raise ValueError("GDAL could not open output")
        srs = dataset.GetSpatialRef()
        expected_srs = osr.SpatialReference()
        expected_srs.ImportFromEPSG(32617)
        if srs is not None:
            srs.SetAxisMappingStrategy(osr.OAMS_TRADITIONAL_GIS_ORDER)
        expected_srs.SetAxisMappingStrategy(osr.OAMS_TRADITIONAL_GIS_ORDER)
        result["dataset"] = {"width": dataset.RasterXSize, "height": dataset.RasterYSize, "bands": dataset.RasterCount,
                             "geotransform": dataset.GetGeoTransform(), "projection_wkt": dataset.GetProjection(),
                             "authority_name": srs.GetAuthorityName(None) if srs else None,
                             "authority_code": srs.GetAuthorityCode(None) if srs else None,
                             "metadata": dataset.GetMetadata(), "image_structure": dataset.GetMetadata("IMAGE_STRUCTURE"), "files": dataset.GetFileList()}
        check(checks, "gdal.dimensions", [dataset.RasterXSize, dataset.RasterYSize] == [width, height], expected=[width, height], actual=[dataset.RasterXSize, dataset.RasterYSize])
        check(checks, "gdal.geotransform", tuple(dataset.GetGeoTransform()) == GEOTRANSFORM, expected=GEOTRANSFORM, actual=dataset.GetGeoTransform())
        check(checks, "gdal.epsg", srs is not None and bool(srs.IsSame(expected_srs)), expected=32617, actual=result["dataset"]["authority_code"])
        check(checks, "gdal.band_count", dataset.RasterCount == len(source), expected=len(source), actual=dataset.RasterCount)
        result["bands"] = []
        for index in range(dataset.RasterCount):
            band = dataset.GetRasterBand(index + 1)
            name = f"band[{index + 1}]"
            entry = {"index": index + 1, "overviews": []}
            result["bands"].append(entry)
            try:
                if index < len(source):
                    entry["full"] = verify_band(band, source[index], case, nodata, interpretations[index], name + ".full", checks, max_examples, False)
                else:
                    entry["full"] = band_metadata(band)
                check(checks, name + ".overview_count", band.GetOverviewCount() == len(dimensions), expected=len(dimensions), actual=band.GetOverviewCount())
                for level in range(band.GetOverviewCount()):
                    overview = band.GetOverview(level)
                    label = name + f".overview[{level}]"
                    record = {"level": level}
                    entry["overviews"].append(record)
                    try:
                        if level < len(dimensions) and index < len(source):
                            w, h, factor = dimensions[level]
                            check(checks, label + ".odd_edge_dimensions", [overview.XSize, overview.YSize] == [w, h], expected=[w, h], actual=[overview.XSize, overview.YSize], full_source_factor=factor)
                            expected = expected_overview(source, case, nodata, factor)[index]
                            record.update(verify_band(overview, expected, case, nodata, interpretations[index], label, checks, max_examples, True))
                        else:
                            record.update(band_metadata(overview))
                            # Read even unexpected overviews so decoder errors are evidence too.
                            pixels = overview.ReadAsArray()
                            record["read_samples"] = int(pixels.size) if pixels is not None else 0
                            check(checks, label + ".unexpected", False, actual_dimensions=[overview.XSize, overview.YSize])
                    except Exception:
                        record.update(exception_record())
                        check(checks, label + ".inspect", False, **exception_record())
            except Exception:
                entry.update(exception_record())
                check(checks, name + ".inspect", False, **exception_record())
        dataset = None
    except Exception:
        check(checks, "gdal.inspect", False, **exception_record())
    result["status"] = "FAIL" if any(c["status"] == "FAIL" for c in checks) else "PASS"
    result["failed_checks"] = sum(c["status"] == "FAIL" for c in checks)
    return result


def parse_export_stdout(stdout):
    value = json.loads(stdout)
    if not isinstance(value, dict):
        raise ValueError("export stdout must be one JSON object")
    elapsed = value.get("export_ms")
    if isinstance(elapsed, bool) or not isinstance(elapsed, (int, float)) or not math.isfinite(elapsed) or elapsed < 0:
        raise ValueError("export_ms must be a finite nonnegative number")
    return value


def export(binary, writer, case, width, height, output, timed, timeout):
    output.parent.mkdir(parents=True, exist_ok=True)
    command = [str(binary), "export", writer, case, str(width), str(height), str(output)]
    if case == "bigtiff":
        command.append("big")
    timing_path = output.with_suffix(".time.txt")
    time_command = []
    if timed:
        if platform.system() == "Darwin":
            time_command = ["/usr/bin/time", "-l", "-o", str(timing_path)]
        elif platform.system() == "Linux":
            time_command = ["/usr/bin/time", "-v", "-o", str(timing_path)]
        else:
            raise ValueError("peak RSS measurement requires macOS or Linux /usr/bin/time")
    record = {"writer": writer, "case": case, "width": width, "height": height,
              "path": str(output), "command": time_command + command, "checks": []}
    start = time.perf_counter()
    try:
        process = subprocess.run(time_command + command, capture_output=True, text=True, timeout=timeout, check=False)
        record.update(returncode=process.returncode, stdout=process.stdout, stderr=process.stderr)
        check(record["checks"], "process.exit", process.returncode == 0, actual=process.returncode, expected=0)
        try:
            data = parse_export_stdout(process.stdout)
            record["export"] = data
            record["export_ms"] = data["export_ms"]
            expected = {"writer": writer, "case": case, "width": width, "height": height}
            for field, wanted in expected.items():
                check(record["checks"], "protocol." + field, data.get(field) == wanted, expected=wanted, actual=data.get(field))
        except Exception:
            check(record["checks"], "protocol.json", False, **exception_record())
        if output.is_file():
            record["bytes"] = output.stat().st_size
            check(record["checks"], "protocol.bytes", record.get("export", {}).get("bytes") == record["bytes"], expected=record["bytes"], actual=record.get("export", {}).get("bytes"))
        else:
            check(record["checks"], "output.exists", False)
    except Exception:
        record.update(exception_record())
        check(record["checks"], "process.launch", False, **exception_record())
    record["process_wall_ms"] = (time.perf_counter() - start) * 1000
    if timed:
        raw = timing_path.read_text() if timing_path.exists() else ""
        record.update(time_raw=raw, time_raw_path=str(timing_path))
        if platform.system() == "Darwin":
            match = re.search(r"^\s*(\d+)\s+maximum resident set size\s*$", raw, re.MULTILINE)
            multiplier = 1
            source_unit = "bytes"
        else:
            match = re.search(r"Maximum resident set size \(kbytes\):\s*(\d+)", raw)
            multiplier = 1024
            source_unit = "KiB"
        record["peak_rss_source_unit"] = source_unit
        record["peak_rss_bytes"] = int(match.group(1)) * multiplier if match else None
        check(record["checks"], "time.peak_rss", match is not None, actual=record["peak_rss_bytes"])
    record["status"] = "FAIL" if any(c["status"] == "FAIL" for c in record["checks"]) else "PASS"
    output.with_suffix(".stdout.txt").write_text(record.get("stdout", ""))
    output.with_suffix(".stderr.txt").write_text(record.get("stderr", ""))
    return record


def save_report(path, report):
    temporary = path.with_suffix(".json.tmp")
    temporary.write_text(json.dumps(json_value(report), indent=2, allow_nan=False) + "\n")
    os.replace(temporary, path)


def performance_summary(runs, repeats, writers):
    result = []
    for case in ("rgb", "float-nodata"):
        for writer in writers:
            selected = [r for r in runs if r["case"] == case and r["writer"] == writer]
            successful = [r for r in selected if r["status"] == "PASS"]
            row = {"case": case, "writer": writer, "expected_runs": repeats, "attempted_runs": len(selected),
                   "successful_runs": len(successful), "failed_runs": len(selected) - len(successful),
                   "status": "PASS" if len(successful) == repeats else "FAIL"}
            for metric in ("export_ms", "process_wall_ms", "peak_rss_bytes", "bytes"):
                values = [r[metric] for r in successful]
                row[metric + "_median"] = statistics.median(values) if values else None
            result.append(row)
    return result


def positive_integer(value):
    number = int(value)
    if number <= 0:
        raise argparse.ArgumentTypeError("must be positive")
    return number


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path, help="release evaluation executable built with the terrio feature")
    parser.add_argument("--out", required=True, type=Path, help="ignored local directory for TIFFs, raw process evidence, and report.json")
    parser.add_argument("--repeats", type=positive_integer, default=3)
    parser.add_argument("--writers", nargs="+", choices=WRITERS, default=list(WRITERS), help="use native alone for the baseline binary built without the terrio feature")
    parser.add_argument("--size", type=positive_integer, default=2048, help="square performance fixtures only; correctness uses rgb 1024x768, others 1025x769")
    parser.add_argument("--phase", choices=("all", "correctness", "performance"), default="all")
    parser.add_argument("--max-examples", type=positive_integer, default=10)
    parser.add_argument("--timeout", type=float, default=None, help="optional per-export timeout in seconds; no default cutoff")
    args = parser.parse_args()
    if args.timeout is not None and (not math.isfinite(args.timeout) or args.timeout <= 0):
        parser.error("--timeout must be finite and positive")
    binary, out = args.binary.resolve(), args.out.resolve()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        parser.error(f"not an executable file: {binary}")
    if out.exists():
        parser.error(f"output directory already exists; select a fresh directory: {out}")
    out.mkdir(parents=True)
    gdal.UseExceptions()
    report = {"schema_version": 1, "started_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "invocation": sys.argv, "binary": str(binary), "platform": platform.platform(), "python": sys.version,
              "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "thread_environment": {k: os.environ.get(k) for k in ("RAYON_NUM_THREADS", "OMP_NUM_THREADS")},
              "gdal": gdal.VersionInfo("--version"), "numpy": np.__version__, "validator_source": cog_validator.__file__,
              "configuration": {"repeats": args.repeats, "writers": args.writers, "performance_size": args.size, "correctness_sizes": {case: ([1024, 768] if case == "rgb" else [1025, 769]) for case in CASES}, "phase": args.phase},
              "measurement_contract": {"export_ms": "File-to-file export: native write_to(BufWriter<File>) or Terrio write_raster_dataset, including sync_all and output metadata; excludes fixture construction.",
                                       "peak_rss": "Fresh export process: fixture, adapter conversion, and writer allocations; native is not forced to buffer a full encoded output Vec. /usr/bin/time reports process peak resident memory.",
                                       "process_wall_ms": "Python subprocess elapsed time, including process startup, fixture construction, file export, and exit; includes time wrapper on performance runs.",
                                       "bytes": "Actual output file length, checked against binary stdout JSON.",
                                       "overview_oracle": "Direct full-source factor-2^n clipped blocks; ceil dimensions until max <=512; nodata excluded; alpha-weighted colors and mean alpha; categorical full-block center; f64 mean to f32 then integer round half-up.",
                                       "correctness": "Exact sample equality (including float32), no numerical or performance acceptance thresholds. Validator warnings retained, errors fail. All cases and both writers attempted despite failures.",
                                       "deflate_level": "Writer configuration is level 6; TIFF compression tags verify Deflate, but the zlib stream does not uniquely prove the configured compression level.",
                                       "performance_order": "Sequential alternating native/Terrio writers each repetition; no concurrent exports or warmup omission."},
              "correctness": [], "performance_runs": [], "performance_summary": []}
    report_path = out / "report.json"
    save_report(report_path, report)
    if args.phase in ("all", "correctness"):
        for case in CASES:
            width, height = (1024, 768) if case == "rgb" else (1025, 769)
            for writer in args.writers:
                output = out / "correctness" / f"{writer}-{case}-{width}x{height}.tif"
                entry = export(binary, writer, case, width, height, output, False, args.timeout)
                if output.is_file():
                    try:
                        entry["verification"] = verify_file(output, case, width, height, args.max_examples)
                    except Exception:
                        entry["verification"] = {"status": "FAIL", **exception_record()}
                    if entry["verification"]["status"] == "FAIL":
                        entry["status"] = "FAIL"
                report["correctness"].append(entry)
                save_report(report_path, report)
                print(f"correctness {writer} {case}: {entry['status']}", flush=True)
    if args.phase in ("all", "performance"):
        for case in ("rgb", "float-nodata"):
            for repeat in range(args.repeats):
                for writer in args.writers:
                    output = out / "performance" / f"{writer}-{case}-{args.size}x{args.size}-run{repeat + 1}.tif"
                    entry = export(binary, writer, case, args.size, args.size, output, True, args.timeout)
                    entry["repeat"] = repeat + 1
                    entry["sequence"] = len(report["performance_runs"]) + 1
                    report["performance_runs"].append(entry)
                    save_report(report_path, report)
                    print(f"performance {writer} {case} run {repeat + 1}: {entry['status']}", flush=True)
        report["performance_summary"] = performance_summary(report["performance_runs"], args.repeats, args.writers)
        # Validate the larger benchmark inputs after all timed runs, never inside timing.
        report["performance_correctness"] = []
        for run in report["performance_runs"]:
            if run["repeat"] != 1:
                continue
            output = Path(run["path"])
            if output.is_file():
                verification = verify_file(output, run["case"], args.size, args.size, args.max_examples)
            else:
                verification = {"status": "FAIL", "error": "benchmark output missing"}
            report["performance_correctness"].append({"writer": run["writer"], "case": run["case"], **verification})
            save_report(report_path, report)
    report["finished_utc"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    entries = report["correctness"] + report["performance_runs"] + report.get("performance_correctness", [])
    report["status"] = "FAIL" if any(e["status"] == "FAIL" for e in entries) else "PASS"
    report["summary"] = {"correctness_pass": sum(e["status"] == "PASS" for e in report["correctness"]),
                         "correctness_fail": sum(e["status"] == "FAIL" for e in report["correctness"]),
                         "performance_pass": sum(e["status"] == "PASS" for e in report["performance_runs"]),
                         "performance_fail": sum(e["status"] == "FAIL" for e in report["performance_runs"]),
                         "benchmark_semantic_fail": sum(e["status"] == "FAIL" for e in report.get("performance_correctness", []))}
    save_report(report_path, report)
    print(json.dumps({"status": report["status"], "report": str(report_path), **report["summary"]}))
    return 1 if report["status"] == "FAIL" else 0


if __name__ == "__main__":
    sys.exit(main())
