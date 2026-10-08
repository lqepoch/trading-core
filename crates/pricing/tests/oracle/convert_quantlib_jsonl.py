#!/usr/bin/env python3
"""Convert synthetic QuantLib JSONL output into the pricing crate CSV fixture.
将 QuantLib 合成 JSONL 输出转换为 pricing crate 的 CSV 固定样本。
"""

import csv
import json
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
    "cash_dividend",
    "cash_dividend_offset_ms",
    "t_grid",
    "x_grid",
    "price",
    "delta",
    "gamma",
    "theta",
)


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} OUTPUT.jsonl", file=sys.stderr)
        return 2

    rows = []
    with open(sys.argv[1], encoding="utf-8") as source:
        for line_number, line in enumerate(source, start=1):
            record = json.loads(line)
            if record.get("kind") != "price":
                raise ValueError(f"oracle record {line_number} is not a price row: {record}")
            if record.get("quantlib") != "1.43":
                raise ValueError(f"oracle record {line_number} has an unexpected QuantLib version")
            rows.append(record)

    if len(rows) != 144:
        raise ValueError(f"expected 144 QuantLib grid rows, received {len(rows)}")

    writer = csv.DictWriter(sys.stdout, fieldnames=COLUMNS, extrasaction="raise", lineterminator="\n")
    writer.writeheader()
    writer.writerows(rows)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
