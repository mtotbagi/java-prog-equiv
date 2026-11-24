"""Plots of `eqbench analyse` results.

    eqbench-plot [--out DIR] <results.jsonl>...

Each results file holds the run of one tool (`eqbench analyse --output`),
named `<tool>-<timeout>.jsonl`. For every timeout found, writes to DIR, in a
light and a dark (`-dark`) variant:

- answers-<timeout>.svg     correct and wrong answers within a time limit,
- quality-<timeout>.svg     what each tool answered on the 248 pairs,
- memory-<timeout>.svg      peak memory over the pairs,
- benchmarks-<timeout>.svg  correct answers per benchmark,

and summary-<timeout>.md, the scores of every tool as a markdown table.

Since a tool that answers in t seconds gives the same answer with any larger
timeout, the answers within a time limit come from a single run.
"""

import argparse
import json
import math
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402
from matplotlib.colors import LinearSegmentedColormap  # noqa: E402
from matplotlib.ticker import MaxNLocator  # noqa: E402

# Tools in their fixed categorical slot: a tool keeps its color in every plot.
# The palette has 8 slots: with more tools, lines go to one panel per tool.
TOOLS = {
    "prog-equiv": "prog-equiv",
    "pasda": "PASDA",
    "ardiff": "ARDiff",
    "dse": "DSE",
    "jbmc": "JBMC",
    "java-ranger": "Java Ranger",
    "jazzer": "Jazzer",
    "imp-s": "IMP-S",
    "prv": "PRV",
}

THEMES = {
    "light": {
        "surface": "#fcfcfb",
        "ink": "#0b0b0b",
        "secondary": "#52514e",
        "muted": "#898781",
        "grid": "#e1e0d9",
        "axis": "#c3c2b7",
        "series": ["#2a78d6", "#eb6834", "#1baf7a", "#eda100", "#e87ba4", "#008300", "#6250d6", "#e34948"],
        "ramp": ["#cde2fb", "#9ec5f4", "#6da7ec", "#3987e5", "#256abf", "#184f95", "#0d366b"],
        "none": "#c3c2b7",
        "error": "#52514e",
    },
    "dark": {
        "surface": "#1a1a19",
        "ink": "#f0efec",
        "secondary": "#c3c2b7",
        "muted": "#898781",
        "grid": "#2c2c2a",
        "axis": "#383835",
        "series": ["#3987e5", "#d95926", "#199e70", "#c98500", "#d55181", "#0ca30c", "#9085e9", "#e66767"],
        "ramp": ["#0d366b", "#104281", "#184f95", "#1c5cab", "#2a78d6", "#5598e7", "#86b6ef"],
        "none": "#52514e",
        "error": "#898781",
    },
}

# Outcomes as states, in the order they are stacked.
STATES = [
    ("correct", "good", "#0ca30c"),
    ("wrong", "critical", "#d03b3b"),
    ("unknown", "none", None),
    ("timeout", "warning", "#fab219"),
    ("memory out", "serious", "#ec835a"),
    ("error", "error", None),
]


@dataclass
class Record:
    id: str
    expected: str
    outcome: str
    seconds: float
    memory_mib: float | None

    @property
    def bench(self) -> str:
        return self.id.split("-", 1)[0]

    @property
    def state(self) -> str:
        if self.outcome in ("eq", "neq"):
            return "correct" if self.outcome == self.expected else "wrong"
        if self.outcome in ("unknown", "timeout", "memory out"):
            return self.outcome
        return "error"


@dataclass
class Run:
    name: str
    label: str
    timeout: int
    jobs: int
    records: list[Record]

    def count(self, state: str) -> int:
        return sum(r.state == state for r in self.records)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=Path, default=Path("eqbench/results/plots"))
    parser.add_argument("files", type=Path, nargs="+")
    args = parser.parse_args()

    by_timeout = defaultdict(list)
    for path in args.files:
        run = load(path)
        by_timeout[run.timeout].append(run)
    args.out.mkdir(parents=True, exist_ok=True)
    for timeout, runs in sorted(by_timeout.items()):
        runs.sort(key=lambda r: list(TOOLS).index(r.name) if r.name in TOOLS else len(TOOLS))
        colors = assign_colors(runs)
        for mode, theme in THEMES.items():
            suffix = "" if mode == "light" else "-dark"
            with plt.rc_context(style(theme)):
                save(answers(runs, colors, theme), args.out / f"answers-{timeout}{suffix}.svg", theme)
                save(quality(runs, theme), args.out / f"quality-{timeout}{suffix}.svg", theme)
                save(memory(runs, colors, theme), args.out / f"memory-{timeout}{suffix}.svg", theme)
                save(benchmarks(runs, theme), args.out / f"benchmarks-{timeout}{suffix}.svg", theme)
        (args.out / f"summary-{timeout}.md").write_text(summary(runs))
        print(f"{timeout} s: {', '.join(r.label for r in runs)} -> {args.out}")


def load(path: Path) -> Run:
    lines = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    if not lines:
        raise SystemExit(f"{path}: no results")
    name = path.stem.rsplit("-", 1)[0] if path.stem.rsplit("-", 1)[-1].isdigit() else path.stem
    records = [Record(r["id"], r["expected"], r["outcome"], r["seconds"], r.get("memory_mib")) for r in lines]
    return Run(name, TOOLS.get(name, name), lines[0]["timeout"], lines[0]["jobs"], records)


def assign_colors(runs: list[Run]) -> dict[str, int] | None:
    """Categorical slot of each tool: its fixed one, else the first free one.
    `None` above 8 tools, the size of the palette."""
    if len(runs) > 8:
        return None
    fixed = {r.name: list(TOOLS).index(r.name) for r in runs if list(TOOLS).index(r.name) < 8} if all(
        r.name in TOOLS for r in runs) else {}
    slots = dict(fixed)
    free = (s for s in range(8) if s not in slots.values())
    for run in runs:
        if run.name not in slots:
            slots[run.name] = next(free)
    return slots


def style(theme: dict) -> dict:
    return {
        "svg.fonttype": "none",
        "font.family": ["Helvetica Neue", "Helvetica", "Arial", "DejaVu Sans"],
        "font.size": 10,
        "figure.facecolor": theme["surface"],
        "axes.facecolor": theme["surface"],
        "axes.edgecolor": theme["axis"],
        "axes.labelcolor": theme["secondary"],
        "axes.titlecolor": theme["ink"],
        "axes.titlesize": 11,
        "axes.titleweight": "bold",
        "axes.titlelocation": "left",
        "axes.spines.top": False,
        "axes.spines.right": False,
        "axes.grid": True,
        "grid.color": theme["grid"],
        "grid.linewidth": 0.8,
        "xtick.color": theme["muted"],
        "ytick.color": theme["muted"],
        "xtick.labelcolor": theme["secondary"],
        "ytick.labelcolor": theme["secondary"],
        "legend.frameon": False,
        "legend.labelcolor": theme["ink"],
        "text.color": theme["ink"],
    }


def save(fig, path: Path, theme: dict) -> None:
    fig.savefig(path, facecolor=theme["surface"], bbox_inches="tight", pad_inches=0.15)
    plt.close(fig)


def subtitle(fig, runs: list[Run], theme: dict) -> None:
    jobs = sorted({r.jobs for r in runs})
    pairs = max(len(r.records) for r in runs)
    fig.text(0.0, 1.0, f"{pairs} pairs, timeout {runs[0].timeout} s, {'/'.join(map(str, jobs))} jobs",
             color=theme["secondary"], ha="left", va="bottom", transform=fig.transFigure)


def answers(runs: list[Run], colors: dict | None, theme: dict):
    if colors is None:
        return answers_per_tool(runs, theme)
    fig, axes = plt.subplots(1, 2, figsize=(10, 4), sharex=True)
    timeout = runs[0].timeout
    labels = []
    for ax, state, title in ((axes[0], "correct", "Correct answers"), (axes[1], "wrong", "Wrong answers")):
        ends = []
        for run in runs:
            times = sorted(r.seconds for r in run.records if r.state == state)
            xs, ys = [0.01], [0]
            for i, t in enumerate(times, 1):
                xs.append(max(t, 0.01))
                ys.append(i)
            xs.append(timeout)
            ys.append(len(times))
            color = theme["series"][colors[run.name]]
            ax.step(xs, ys, where="post", color=color, linewidth=2, label=run.label)
            ends.append((len(times), run.label, color))
        ax.set_xscale("log")
        ax.set_xlim(0.1, timeout)
        ax.set_ylim(0, max(1, max(y for y, _, _ in ends)) * 1.08)
        ax.yaxis.set_major_locator(MaxNLocator(integer=True))
        ax.set_title(title)
        ax.set_xlabel("time limit (s)")
        labels.append((ax, ends))
    axes[0].set_ylabel("pairs")
    subtitle(fig, runs, theme)
    # Room between the panels for the direct labels, which the layout ignores.
    fig.tight_layout(w_pad=3)
    # Below the panels: in a corner, the legend would hide the curves that rise first.
    handles, names = axes[0].get_legend_handles_labels()
    fig.legend(handles, names, loc="upper center", bbox_to_anchor=(0.5, 0), ncols=min(len(runs), 4))
    for ax, ends in labels:
        direct_labels(ax, ends, timeout, theme)
    return fig


def answers_per_tool(runs: list[Run], theme: dict):
    """Small multiples: one panel per tool, correct and wrong answers."""
    columns = 3
    rows = math.ceil(len(runs) / columns)
    fig, axes = plt.subplots(rows, columns, figsize=(10, 2.6 * rows), sharex=True, sharey=True, squeeze=False)
    timeout = runs[0].timeout
    top = max(max(run.count("correct"), run.count("wrong")) for run in runs)
    labels = []
    for ax, run in zip(axes.flat, runs):
        ends = []
        for state, color in (("correct", "#0ca30c"), ("wrong", "#d03b3b")):
            times = sorted(r.seconds for r in run.records if r.state == state)
            ax.step([0.01, *[max(t, 0.01) for t in times], timeout], [0, *range(1, len(times) + 1), len(times)],
                    where="post", color=color, linewidth=2, label=state)
            ends.append((len(times), state, color))
        ax.set_title(run.label)
        labels.append((ax, ends))
    for ax in axes.flat[len(runs):]:
        ax.set_visible(False)
    axes[0][0].set_xscale("log")
    axes[0][0].set_xlim(0.1, timeout)
    axes[0][0].set_ylim(0, max(1, top) * 1.08)
    axes[0][0].yaxis.set_major_locator(MaxNLocator(integer=True))
    for ax in axes[-1]:
        ax.set_xlabel("time limit (s)")
    for row in axes:
        row[0].set_ylabel("pairs")
    axes[0][0].legend(loc="upper left")
    subtitle(fig, runs, theme)
    fig.tight_layout(w_pad=3)
    for ax, ends in labels:
        direct_labels(ax, ends, timeout, theme)
    return fig


def direct_labels(ax, ends: list, x: float, theme: dict) -> None:
    """Count of each line at its right end, labels nudged 12 points apart.

    Called once the limits and the layout are final: the nudge is computed
    from where the ends are drawn.
    """
    to_points = 72 / ax.figure.dpi
    previous = None
    for y, _, color in sorted(ends):
        at = ax.transData.transform((x, y))[1] * to_points
        shift = 0.0 if previous is None else max(0.0, previous + 12 - at)
        previous = at + shift
        ax.plot([x], [y], marker="o", markersize=8, color=color, markeredgecolor=theme["surface"],
                markeredgewidth=2, clip_on=False, zorder=5)
        ax.annotate(str(y), xy=(x, y), xytext=(8, shift), textcoords="offset points", va="center",
                    color=theme["ink"], fontsize=9, annotation_clip=False)


def quality(runs: list[Run], theme: dict):
    ordered = sorted(runs, key=lambda r: (r.count("correct"), -r.count("wrong")))
    fig, ax = plt.subplots(figsize=(10, 0.55 * len(runs) + 1.4))
    for row, run in enumerate(ordered):
        left = 0
        for state, role, color in STATES:
            n = run.count(state)
            if not n:
                continue
            fill = color or theme[role]
            ax.barh(row, n, left=left, color=fill, edgecolor=theme["surface"], linewidth=2, height=0.62)
            if n >= 9:
                ink = "#0b0b0b" if state == "timeout" else "#ffffff"
                ax.text(left + n / 2, row, str(n), ha="center", va="center", color=ink, fontsize=9)
            left += n
    ax.set_yticks(range(len(ordered)), [r.label for r in ordered])
    ax.set_xlim(0, max(len(r.records) for r in runs))
    ax.set_xlabel("pairs")
    ax.set_axisbelow(True)
    ax.grid(axis="y", visible=False)
    ax.tick_params(axis="y", length=0)
    ax.set_title("What each tool answered", pad=30)
    handles = [plt.Rectangle((0, 0), 1, 1, color=c or theme[role]) for _, role, c in STATES]
    ax.legend(handles, [s for s, _, _ in STATES], ncols=len(STATES), loc="lower left",
              bbox_to_anchor=(0, 1.0), handlelength=1, handleheight=1, columnspacing=1.2)
    subtitle(fig, runs, theme)
    fig.tight_layout()
    return fig


def memory(runs: list[Run], colors: dict | None, theme: dict):
    if colors is None:
        columns = 3
        rows = math.ceil(len(runs) / columns)
        fig, axes = plt.subplots(rows, columns, figsize=(10, 2.6 * rows), sharex=True, sharey=True, squeeze=False)
        for ax, run in zip(axes.flat, runs):
            memory_line(ax, run, theme["series"][0])
            ax.set_title(run.label)
        for ax in axes.flat[len(runs):]:
            ax.set_visible(False)
        for ax in axes[-1]:
            ax.set_xlabel("peak memory (MiB)")
        for row in axes:
            row[0].set_ylabel("pairs (%)")
    else:
        fig, ax = plt.subplots(figsize=(7, 4))
        for run in runs:
            memory_line(ax, run, theme["series"][colors[run.name]], label=run.label)
        ax.set_xlabel("peak resident memory (MiB)")
        ax.set_ylabel("pairs (%)")
        ax.set_title("Pairs analysed within a memory size")
        ax.legend(loc="upper left")
        axes = [[ax]]
    axes[0][0].set_xscale("log")
    axes[0][0].set_ylim(0, 100)
    subtitle(fig, runs, theme)
    fig.tight_layout()
    return fig


def memory_line(ax, run: Run, color: str, label: str | None = None) -> None:
    """Share of the pairs whose peak memory is below each size."""
    peaks = sorted(r.memory_mib for r in run.records if r.memory_mib)
    if peaks:
        ys = [100 * i / len(run.records) for i in range(1, len(peaks) + 1)]
        ax.step(peaks, ys, where="post", color=color, linewidth=2, label=label)


def benchmarks(runs: list[Run], theme: dict):
    benches = sorted({r.bench for run in runs for r in run.records}, key=str.lower)
    fig, ax = plt.subplots(figsize=(1.1 * len(runs) + 2.2, 0.36 * len(benches) + 1.2))
    cmap = LinearSegmentedColormap.from_list("ramp", theme["ramp"])
    for col, run in enumerate(runs):
        for row, bench in enumerate(benches):
            cases = [r for r in run.records if r.bench == bench]
            correct = sum(r.state == "correct" for r in cases)
            share = correct / len(cases) if cases else 0
            ax.add_patch(plt.Rectangle((col, row), 1, 1, color=cmap(share), ec=theme["surface"], lw=2))
            ink = "#ffffff" if luminance(cmap(share)) < 0.45 else "#0b0b0b"
            ax.text(col + 0.5, row + 0.5, f"{correct}/{len(cases)}", ha="center", va="center", color=ink, fontsize=8)
    ax.set_xlim(0, len(runs))
    ax.set_ylim(len(benches), 0)
    ax.set_xticks([c + 0.5 for c in range(len(runs))], [r.label for r in runs])
    ax.set_yticks([r + 0.5 for r in range(len(benches))], benches)
    ax.tick_params(length=0)
    ax.xaxis.tick_top()
    ax.grid(False)
    for spine in ax.spines.values():
        spine.set_visible(False)
    ax.set_title("Correct answers per benchmark", pad=28)
    subtitle(fig, runs, theme)
    fig.tight_layout()
    return fig


def luminance(rgba) -> float:
    r, g, b = (c if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4 for c in rgba[:3])
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def summary(runs: list[Run]) -> str:
    rows = [
        "| Tool | Correct | F1 | eq precision / recall | neq precision / recall | unknown | timeout | memory out | error | Mean time | Max memory |",
        "|---|---|---|---|---|---|---|---|---|---|---|",
    ]
    for run in sorted(runs, key=lambda r: -r.count("correct")):
        scores = {kind: score(run.records, kind) for kind in ("eq", "neq")}
        f1 = (scores["eq"][2] + scores["neq"][2]) / 2
        mean = sum(r.seconds for r in run.records) / len(run.records)
        peaks = [r.memory_mib for r in run.records if r.memory_mib]
        rows.append(
            f"| {run.label} | {run.count('correct')}/{len(run.records)} | {f1:.3f} "
            f"| {ratio(scores['eq'][0])} / {ratio(scores['eq'][1])} "
            f"| {ratio(scores['neq'][0])} / {ratio(scores['neq'][1])} "
            f"| {run.count('unknown')} | {run.count('timeout')} | {run.count('memory out')} | {run.count('error')} "
            f"| {mean:.1f} s | {max(peaks) if peaks else 0:.0f} MiB |"
        )
    return "\n".join(rows) + "\n"


def score(records: list[Record], kind: str) -> tuple[float | None, float | None, float]:
    """Precision, recall and F1 of `kind`, as `eqbench analyse` computes them."""
    actual = sum(r.expected == kind for r in records)
    predicted = sum(r.outcome == kind for r in records)
    correct = sum(r.expected == kind and r.outcome == kind for r in records)
    precision = correct / predicted if predicted else None
    recall = correct / actual if actual else None
    f1 = 2 * correct / (predicted + actual) if predicted + actual else 0.0
    return precision, recall, f1


def ratio(value: float | None) -> str:
    return "-" if value is None or math.isnan(value) else f"{value:.3f}"


if __name__ == "__main__":
    main()
