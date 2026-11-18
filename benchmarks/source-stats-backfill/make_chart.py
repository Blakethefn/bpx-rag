"""Render source-stats-backfill.png from results.json.

Usage: python3 make_chart.py   (writes source-stats-backfill.png next to this file)
"""
import json
import math
from pathlib import Path

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

HERE = Path(__file__).resolve().parent
DATA = json.loads((HERE / "results.json").read_text())
OUT = HERE / "source-stats-backfill.png"

INK, MUTED, GRID = '#1F2933', '#6B7280', '#E5E7EB'
ACCENT, NEUTRAL = '#1F6FB2', '#B8C2CC'

plt.rcParams.update({
    'font.family': 'DejaVu Sans',
    'font.size': 11,
    'text.color': INK,
    'axes.labelcolor': INK,
    'xtick.color': MUTED,
    'ytick.color': INK,
})

DATASET_LABELS = {"small": "1k chunks", "large": "1M chunks"}
datasets = DATA["datasets"]
order = [k for k in ("small", "large") if k in datasets]
runs = datasets[order[0]]["conditions"]["cold"]["runs_per_variant"]
sources = datasets[order[0]]["dataset"]["sources"]

rows = []  # (label, median seconds or None for a header, color, group)
for key in order:
    ds = datasets[key]
    rows.append((DATASET_LABELS.get(key, key), None, None, key))
    for cond in ("cold", "warm"):
        cname = "cold cache" if cond == "cold" else "warm cache"
        variants = ds["conditions"][cond]["variants"]
        rows.append((f"Before · {cname}", variants["before"]["median"], NEUTRAL, key))
        rows.append((f"After · {cname}", variants["after"]["median"], ACCENT, key))

fig = plt.figure(figsize=(12.6, 7.4), facecolor='white')
gs = fig.add_gridspec(1, 2, width_ratios=[1.55, 1], wspace=0.34,
                      left=0.22, right=0.96, top=0.77, bottom=0.24)
ax = fig.add_subplot(gs[0, 0])
ax2 = fig.add_subplot(gs[0, 1])

fig.text(0.06, 0.95, 'source_stats benchmark', fontsize=19, fontweight='bold', color=INK)
fig.text(0.06, 0.905, f'Synthetic databases, {sources} sources each · small and large dataset',
         fontsize=11, color=MUTED)

y_positions = []
y = 0
prev_group = None
for label, value, color, group in rows:
    if prev_group is not None and group != prev_group:
        y += 0.5  # gap between dataset groups
    y_positions.append(y)
    prev_group = group
    y += 1

bars = [(yp, r) for yp, r in zip(y_positions, rows) if r[1] is not None]
values = [r[1] for _, r in bars]
ax.barh([yp for yp, _ in bars], values, height=0.6, color=[r[2] for _, r in bars], edgecolor='none')
ax.invert_yaxis()
ax.set_yticks(y_positions, [r[0] for r in rows])
for tick, r in zip(ax.get_yticklabels(), rows):
    if r[1] is None:
        tick.set_fontweight('bold')
        tick.set_color(INK)
ax.tick_params(axis='y', length=0, pad=10)
ax.set_xlim(0, max(values) * 1.18)
ax.set_xlabel('Seconds (median of runs)', fontsize=10, color=MUTED, labelpad=10)
ax.xaxis.grid(True, color=GRID, linewidth=0.8)
ax.set_axisbelow(True)
for spine in ax.spines.values():
    spine.set_visible(False)
for (yp, _), v in zip(bars, values):
    if v >= 10:
        label = f'{v:.1f} s'
    elif v >= 0.1:
        label = f'{v:.2f} s'
    elif v >= 0.001:
        label = f'{v * 1000:.1f} ms'
    else:
        label = f'{v * 1e6:.0f} µs'
    ax.text(v + max(values) * 0.015, yp, label, ha='left', va='center', fontweight='bold', fontsize=10.5)

fig.text(0.22, 0.83, 'Backfill duration', fontsize=12, fontweight='bold')

ax2.set_axis_off()
ax2.text(0, 1.10, 'Speedup, after vs before', transform=ax2.transAxes,
         fontsize=12, fontweight='bold', ha='left', color=INK)
panel = []
for key in order:
    name = DATASET_LABELS.get(key, key)
    for cond in ("cold", "warm"):
        c = datasets[key]["conditions"][cond]
        panel.append((f"{c['ratio_of_medians']:.0f}×" if c['ratio_of_medians'] >= 10
                      else f"{c['ratio_of_medians']:.1f}×",
                      f"{name} · {'cold' if cond == 'cold' else 'warm'} cache · median of runs"))
positions = [0.86, 0.64, 0.42, 0.20]
for (big, small_label), ypos in zip(panel, positions):
    ax2.text(0, ypos, big, fontsize=26, fontweight='bold', transform=ax2.transAxes, va='top', color=ACCENT)
    ax2.text(0, ypos - 0.13, small_label, fontsize=9.5, transform=ax2.transAxes, va='top', color=MUTED)
ax2.text(0, -0.05, 'Both variants produced identical\nsource_stats rows in every run.',
         fontsize=9.5, transform=ax2.transAxes, va='top', color=MUTED)

fig.text(0.06, 0.13,
         f'{runs} timed runs per variant per condition, alternating order, one process, one working copy. '
         'Each run is rolled back.', fontsize=9, color=MUTED)
fig.text(0.06, 0.09,
         'On this linear scale the 1k bars are too short to see. Their labels carry the values, and the warm times are near the timer noise floor.',
         fontsize=9, color=MUTED)
fig.text(0.06, 0.05,
         'Cold runs evict the working copy from the page cache before each run. '
         'Results depend on the storage device, cache size, and row layout of the machine that ran them.',
         fontsize=9, color=MUTED)

fig.savefig(OUT, dpi=160, facecolor='white')
print(OUT.name)
