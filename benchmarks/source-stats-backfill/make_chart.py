"""Render source-stats-backfill.png from results.json.

Each dataset gets its own panel and its own linear x-axis.
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

DATASET_LABELS = {"small": "1k chunks", "large": "1M chunks"}
datasets = DATA["datasets"]
order = [k for k in ("small", "large") if k in datasets]
runs = datasets[order[0]]["conditions"]["cold"]["runs_per_variant"]
sources = datasets[order[0]]["dataset"]["sources"]


def fmt(v):
    if v >= 10:
        return f"{v:.1f} s"
    if v >= 0.1:
        return f"{v:.2f} s"
    if v >= 0.001:
        return f"{v * 1000:.1f} ms"
    return f"{v * 1e6:.0f} µs"


def bars_for(key):
    ds = datasets[key]
    out = []
    for cond in ("cold", "warm"):
        cname = "cold cache" if cond == "cold" else "warm cache"
        variants = ds["conditions"][cond]["variants"]
        out.append((f"Before · {cname}", variants["before"]["median"], NEUTRAL))
        out.append((f"After · {cname}", variants["after"]["median"], ACCENT))
    return out


def draw(ax, key):
    bars = bars_for(key)
    values = [b[1] for b in bars]
    ax.barh(range(len(bars)), values, height=0.6, color=[b[2] for b in bars], edgecolor='none')
    ax.invert_yaxis()
    ax.set_yticks(range(len(bars)), [b[0] for b in bars])
    ax.tick_params(axis='y', length=0, pad=10)
    top = max(values)
    ax.set_xlim(0, top * 1.22)
    ax.set_xlabel('Seconds (median of runs)', fontsize=10, color=MUTED, labelpad=6)
    ax.xaxis.grid(True, color=GRID, linewidth=0.8)
    ax.set_axisbelow(True)
    for spine in ax.spines.values():
        spine.set_visible(False)
    for i, v in enumerate(values):
        ax.text(v + top * 0.015, i, fmt(v), ha='left', va='center', fontweight='bold', fontsize=10.5)
    ds = datasets[key]["dataset"]
    ax.set_title(f"{ds['chunks']:,} chunks",
                 loc='left', fontsize=12, fontweight='bold', color=INK, pad=8)


fig = plt.figure(figsize=(12.6, 8.2), facecolor='white')
fig.text(0.06, 0.955, 'source_stats benchmark', fontsize=19, fontweight='bold', color=INK)
fig.text(0.06, 0.915, f'Synthetic databases, {sources} sources each · each dataset on its own x-axis',
         fontsize=11, color=MUTED)

# Two stacked panels on the left, each with its own x-axis
panel_rects = {
    order[0]: [0.23, 0.60, 0.42, 0.23],
    order[-1]: [0.23, 0.205, 0.42, 0.23],
}
for key in order:
    ax = fig.add_axes(panel_rects[key])
    draw(ax, key)

# Right: speedup per dataset and cache condition
ax2 = fig.add_axes([0.72, 0.215, 0.25, 0.62])
ax2.set_axis_off()
ax2.text(0, 1.06, 'Speedup, after vs before', transform=ax2.transAxes,
         fontsize=12, fontweight='bold', ha='left', color=INK)
entries = []
for key in order:
    name = DATASET_LABELS.get(key, key)
    for cond in ("cold", "warm"):
        c = datasets[key]["conditions"][cond]
        ratio = c["ratio_of_medians"]
        big = f"{ratio:.0f}×" if ratio >= 10 else f"{ratio:.1f}×"
        entries.append((big, f"{name} · {'cold' if cond == 'cold' else 'warm'} cache · median of runs"))
positions = [0.90, 0.70, 0.46, 0.26]
for (big, label), ypos in zip(entries, positions):
    ax2.text(0, ypos, big, fontsize=26, fontweight='bold', transform=ax2.transAxes, va='top', color=ACCENT)
    ax2.text(0, ypos - 0.12, label, fontsize=9.5, transform=ax2.transAxes, va='top', color=MUTED)
ax2.text(0, -0.02, 'Both variants produced identical\nsource_stats rows in every run.',
         fontsize=9.5, transform=ax2.transAxes, va='top', color=MUTED)

fig.text(0.06, 0.115,
         f'{runs} timed runs per variant per condition, alternating order, one process, one working copy. '
         'Each run is rolled back.', fontsize=9, color=MUTED)
fig.text(0.06, 0.08,
         'The 1k warm times are near the timer noise floor, so treat their ratio with care.',
         fontsize=9, color=MUTED)
fig.text(0.06, 0.045,
         'Cold runs evict the working copy from the page cache before each run. '
         'Results depend on the storage device, cache size, and row layout of the machine that ran them.',
         fontsize=9, color=MUTED)

fig.savefig(OUT, dpi=160, facecolor='white')
print(OUT.name)
