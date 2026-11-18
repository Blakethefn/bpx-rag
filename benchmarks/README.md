# Benchmarks

Public benchmark results for bpx-rag. Each folder is one benchmark: a write-up, a chart, the raw numbers, and the script that produces the chart.

| Benchmark | Question | Headline |
|---|---|---|
| [source-stats-backfill](source-stats-backfill/README.md) | How long does the `source_stats` backfill take before and after the coverage-sum change, on a 1k and a 1M synthetic dataset, cold and warm? | 1k chunks cold: 10.9 ms to 3.6 ms (3.0x); 1k chunks warm: 1.4 ms to 256 µs (5.3x); 1M chunks cold: 22.1 s to 0.46 s (47.6x); 1M chunks warm: 20.5 s to 0.40 s (51.1x) |

## Re-running

```sh
cd benchmarks/<slug>
python3 make_chart.py
```

Each `make_chart.py` needs only matplotlib. The timings in `results.json` come from the commands listed in that benchmark's README.
