# source-stats-backfill

## Question

How long does the `source_stats` backfill take when its embedded counts come from a sum over `knowledge_embedding_coverage` (after) instead of a per-chunk embedding count (before), under cold and warm cache?

## Result

![source_stats backfill](source-stats-backfill.png)

- Cold cache, median of 3 runs: before 81.7 s, after 6.08 s. Speedup 13.4x.
- Warm cache, median of 3 runs: before 0.123 s, after 0.008 s. Speedup 14.7x.
- Both variants wrote identical `source_stats` rows in every run.

## Method

- **Harness.** `harness.py` runs both variants in one process against one working copy of the database. Order alternates between runs. Each run is rolled back, so the working copy never changes.
- **Variants.** *before* uses correlated scalar subqueries per source. *after* uses `LEFT JOIN` to grouped aggregates, with embedded counts summed from `knowledge_embedding_coverage`. The SQL for both is in `harness.py`.
- **Cold condition.** The working copy is evicted from the page cache before every timed run with `posix_fadvise(POSIX_FADV_DONTNEED)`. This needs a Unix that provides it.
- **Warm condition.** One untimed warm-up run per variant, then timed runs with the cache populated.
- **Correctness.** Each variant's resulting `source_stats` rows are hashed, and the harness fails if the two hashes differ.
- **Runs.** 3 timed runs per variant per condition. The chart shows medians. `results.json` keeps every run.

## Dataset

- 124,880 chunks, 50 sources, 11,837 files, 124,880 embedding rows.
- Database size 1,968,582,656 bytes.
- The database is not published. Re-measuring needs any bpx-rag database of similar shape.

## Environment

- Operating system: Linux (x86_64).
- Python 3.12.3, SQLite 3.45.1.
- Logical CPUs: 12.
- Storage device and memory size were not standardized. Cold results depend on both.

## Caveats

- **Cold results depend on storage.** The old per-chunk probe reads embedding rows at random. On fast storage the cold gap will be smaller than measured here. The warm gap is CPU-bound and more stable.
- **One dataset.** A single database shape. Other shapes will change the ratio.
- **One machine.** Results have not been reproduced on other hardware.
- **Coverage dependency.** The after variant assumes `knowledge_embedding_coverage` matches the embedding rows. The trigger keeps it in step, and the correctness check above compares the output of both variants on the same copy.

## Reproduce

```sh
cd benchmarks/source-stats-backfill
python3 harness.py --db PATH/TO/knowledge.db --cache cold --runs 3 --out cold.json
python3 harness.py --db PATH/TO/knowledge.db --cache warm --runs 3 --out warm.json
python3 combine.py cold.json warm.json   # writes results.json
python3 make_chart.py                    # writes source-stats-backfill.png
```
