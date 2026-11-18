# Benchmarks

Public benchmark results for bpx-rag. Each folder is one benchmark: a write-up, a chart, the raw numbers, and the script that produces the chart.

| Benchmark | Question | Headline |
|---|---|---|
| [source-stats-backfill](source-stats-backfill/README.md) | How long does the `source_stats` backfill take before and after the coverage-sum change, cold and warm? | 81.7 s to 6.08 s cold (13.4x); 0.123 s to 0.008 s warm (14.7x) |

## Re-running

```sh
cd benchmarks/<slug>
python3 make_chart.py
```

Each `make_chart.py` needs only matplotlib. The timings in `results.json` come from the commands listed in that benchmark's README.
