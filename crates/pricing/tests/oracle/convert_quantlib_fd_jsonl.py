#!/usr/bin/env python3
"""Convert synthetic QuantLib finite-difference probes into a CSV fixture.
将 QuantLib 合成有限差分探针转换为 CSV 固定样本。
"""

import csv
import json
import math
import sys

COLUMNS = (
    "kind",
    "id",
    "quantlib",
    "option",
    "t_ms",
    "t_act365f",
    "spot",
    "strike",
    "rate",
    "continuous_yield",
    "sigma",
    "t_grid",
    "x_grid",
    "axis",
    "bump_fraction",
    "bump_value",
    "bump_unit",
    "base_price",
    "positive_axis_price",
    "negative_axis_price",
)

EXPECTED_PROBES = {
    "spot": {0.0001: "underlying-unit", 0.001: "underlying-unit", 0.01: "underlying-unit"},
    "time": {0.001: "millisecond", 0.01: "millisecond", 0.05: "millisecond"},
}


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} OUTPUT.jsonl", file=sys.stderr)
        return 2

    rows = []
    with open(sys.argv[1], encoding="utf-8") as source:
        for line_number, line in enumerate(source, start=1):
            record = json.loads(line)
            if record.get("kind") != "price-probe":
                raise ValueError(f"oracle record {line_number} is not a price probe: {record}")
            if record.get("quantlib") != "1.43":
                raise ValueError(f"oracle record {line_number} has an unexpected QuantLib version")
            if (record.get("t_grid"), record.get("x_grid")) != (1600, 3200):
                raise ValueError(f"oracle record {line_number} has an unexpected grid")
            if record.get("axis") not in EXPECTED_PROBES:
                raise ValueError(f"oracle record {line_number} has an unexpected axis")
            fraction = record.get("bump_fraction")
            if fraction not in EXPECTED_PROBES[record["axis"]]:
                raise ValueError(f"oracle record {line_number} has an unexpected bump fraction")
            if record.get("bump_unit") != EXPECTED_PROBES[record["axis"]][fraction]:
                raise ValueError(f"oracle record {line_number} has an unexpected bump unit")
            for field in (
                "t_act365f",
                "spot",
                "strike",
                "rate",
                "continuous_yield",
                "sigma",
                "bump_value",
                "base_price",
                "positive_axis_price",
                "negative_axis_price",
            ):
                value = record.get(field)
                if not isinstance(value, (int, float)) or not math.isfinite(value):
                    raise ValueError(f"oracle record {line_number} has invalid {field}")
            rows.append(record)

    if len(rows) != 34 * 6:
        raise ValueError(f"expected 204 QuantLib finite-difference probes, received {len(rows)}")
    observed = set()
    case_ids = set()
    for row in rows:
        key = (row["id"], row["axis"], row["bump_fraction"])
        if key in observed:
            raise ValueError(f"duplicate QuantLib finite-difference probe: {key}")
        observed.add(key)
        case_ids.add(row["id"])
    if len(case_ids) != 34:
        raise ValueError(f"expected 34 synthetic cases, received {len(case_ids)}")
    for case_id in case_ids:
        for axis, fractions in EXPECTED_PROBES.items():
            for fraction in fractions:
                if (case_id, axis, fraction) not in observed:
                    raise ValueError(f"missing QuantLib finite-difference probe: {(case_id, axis, fraction)}")

    writer = csv.DictWriter(sys.stdout, fieldnames=COLUMNS, extrasaction="raise", lineterminator="\n")
    writer.writeheader()
    writer.writerows(rows)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
