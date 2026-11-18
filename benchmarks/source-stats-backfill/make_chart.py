"""Render source-stats-backfill.png from results.json.

Usage: python3 make_chart.py   (writes source-stats-backfill.png next to this file)
"""
import json
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

cold = DATA["conditions"]["cold"]
warm = DATA["conditions"]["warm"]
chunks = DATA["dataset"]["chunks"]
sources = DATA["dataset"]["sources"]
runs = cold["runs_per_variant"]

fig = plt.figure(figsize=(12.6, 5.8), facecolor='white')
gs = fig.add_gridspec(1, 2, width_ratios=[1.55, 1], wspace=0.29,
                      left=0.20, right=0.96, top=0.73, bottom=0.25)
ax = fig.add_subplot(gs[0, 0])
ax2 = fig.add_subplot(gs[0, 1])

fig.text(0.06, 0.93, 'source_stats benchmark', fontsize=19, fontweight='bold', color=INK)
fig.text(0.06, 0.875, f'{chunks:,} chunks across {sources} sources', fontsize=11, color=MUTED)

names = ['Before · cold cache', 'After · cold cache', 'Before · warm cache', 'After · warm cache']
values = [cold['variants']['before']['median'], cold['variants']['after']['median'],
          warm['variants']['before']['median'], warm['variants']['after']['median']]
y = range(len(values))
ax.barh(y, values, height=0.55, color=[NEUTRAL, ACCENT, NEUTRAL, ACCENT], edgecolor='none')
ax.invert_yaxis()
ax.set_yticks(list(y), names)
ax.tick_params(axis='y', length=0, pad=12)
top = max(values)
ax.set_xlim(0, top * 1.18)
ax.set_xlabel('Seconds (median of runs)', fontsize=10, color=MUTED, labelpad=10)
ax.xaxis.grid(True, color=GRID, linewidth=0.8)
ax.set_axisbelow(True)
for spine in ax.spines.values():
    spine.set_visible(False)
for i, v in enumerate(values):
    label = f'{v:.1f} s' if v >= 10 else f'{v:.2f} s'
    ax.text(v + top * 0.02, i, label, ha='left', va='center', fontweight='bold', fontsize=11)

fig.text(0.20, 0.79, 'Backfill duration', fontsize=12, fontweight='bold')
fig.text(0.58, 0.79, f"≈{cold['ratio_of_medians']:.0f}× faster, cold · ≈{warm['ratio_of_medians']:.0f}× warm",
         fontsize=11, fontweight='bold', color=ACCENT, ha='right')

ax2.set_axis_off()
ax2.text(0, 1.10, 'Speedup, after vs before', transform=ax2.transAxes,
         fontsize=12, fontweight='bold', ha='left', color=INK)
ax2.text(0, 0.79, f"{cold['ratio_of_medians']:.0f}×", fontsize=28, fontweight='bold',
         transform=ax2.transAxes, va='top', color=INK)
ax2.text(0, 0.61, 'Cold cache · median of runs', fontsize=10, transform=ax2.transAxes, va='top', color=MUTED)
ax2.plot([0, 0.95], [0.50, 0.50], transform=ax2.transAxes, color=GRID, linewidth=1)
ax2.text(0, 0.39, f"{warm['ratio_of_medians']:.0f}×", fontsize=28, fontweight='bold',
         transform=ax2.transAxes, va='top', color=INK)
ax2.text(0, 0.21, 'Warm cache · median of runs', fontsize=10, transform=ax2.transAxes, va='top', color=MUTED)
ax2.text(0, -0.07, 'Both variants produced identical source_stats rows.',
         fontsize=9.5, transform=ax2.transAxes, va='top', color=MUTED)

fig.text(0.12, 0.105,
         f'{runs} timed runs per variant per condition, alternating order, one process, one working copy. '
         'Each run is rolled back.', fontsize=9, color=MUTED)
fig.text(0.12, 0.065,
         'Cold runs evict the working copy from the page cache before each run. '
         'Results depend on the storage device and cache size of the machine that ran them.',
         fontsize=9, color=MUTED)

fig.savefig(OUT, dpi=160, facecolor='white')
print(OUT.name)
