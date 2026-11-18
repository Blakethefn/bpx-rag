"""Merge harness outputs into results.json, one entry per dataset.

Usage:
  python3 combine.py small=small-cold.json,small-warm.json large=large-cold.json,large-warm.json

Each dataset needs one cold and one warm harness output. Every run must share the
same environment, and the before and after outputs must match in every run.
"""
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def condition(run: dict) -> dict:
    return {
        "cache_controlled": run["cache_controlled"],
        "runs_per_variant": run["runs_per_variant"],
        "variants": run["variants"],
        "ratio_of_medians": run["ratio_of_medians"],
        "outputs_match": run["outputs_match"],
    }


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    datasets, environment = {}, None
    for arg in sys.argv[1:]:
        name, paths = arg.split("=", 1)
        cold_path, warm_path = paths.split(",", 1)
        cold = json.loads(Path(cold_path).read_text())
        warm = json.loads(Path(warm_path).read_text())
        if cold["dataset"] != warm["dataset"]:
            raise SystemExit(f"{name}: cold and warm runs used different datasets")
        for run in (cold, warm):
            if environment is None:
                environment = run["environment"]
            elif run["environment"] != environment:
                raise SystemExit(f"{name}: runs used different environments")
        datasets[name] = {
            "dataset": cold["dataset"],
            "conditions": {"cold": condition(cold), "warm": condition(warm)},
        }
    if not all(c["outputs_match"] for d in datasets.values() for c in d["conditions"].values()):
        raise SystemExit("before and after outputs differ; do not publish these numbers")
    combined = {
        "benchmark": "source-stats-backfill",
        "harness": "harness.py",
        "environment": environment,
        "datasets": datasets,
    }
    (HERE / "results.json").write_text(json.dumps(combined, indent=2) + "\n")
    print("wrote results.json with datasets: " + ", ".join(datasets))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
