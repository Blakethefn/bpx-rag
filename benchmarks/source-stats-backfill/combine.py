"""Merge the cold and warm harness outputs into results.json.

Usage:
  python3 combine.py results-cold.json results-warm.json
"""
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    cold = json.loads(Path(sys.argv[1]).read_text())
    warm = json.loads(Path(sys.argv[2]).read_text())
    if cold["dataset"] != warm["dataset"]:
        raise SystemExit("cold and warm runs used different datasets")
    if cold["environment"] != warm["environment"]:
        raise SystemExit("cold and warm runs used different environments")

    def condition(run: dict) -> dict:
        return {
            "cache_controlled": run["cache_controlled"],
            "runs_per_variant": run["runs_per_variant"],
            "variants": run["variants"],
            "ratio_of_medians": run["ratio_of_medians"],
            "outputs_match": run["outputs_match"],
        }

    combined = {
        "benchmark": "source-stats-backfill",
        "harness": "harness.py",
        "dataset": cold["dataset"],
        "environment": cold["environment"],
        "conditions": {"cold": condition(cold), "warm": condition(warm)},
    }
    if not all(c["outputs_match"] for c in combined["conditions"].values()):
        raise SystemExit("before and after outputs differ; do not publish these numbers")
    (HERE / "results.json").write_text(json.dumps(combined, indent=2) + "\n")
    print("wrote results.json")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
