#!/usr/bin/env python3
"""Measure end-to-end analyze-file latency on a repeatable polyglot fixture."""

from __future__ import annotations

import argparse
import math
import statistics
import subprocess
import tempfile
import time
from pathlib import Path


def fixture_files(root: Path, count: int) -> Path:
    (root / "Cargo.toml").write_text("[package]\nname = 'bench_fixture'\nversion = '0.1.0'\n")
    (root / "go.mod").write_text("module example.com/bench\ngo 1.23\n")
    (root / "pubspec.yaml").write_text("name: bench_fixture\n")
    languages = {
        "markdown": ("md", lambda i: f"See [[markdown/file{(i + 1) % count}]].\n"),
        "rust": ("rs", lambda i: f"use crate::rust::file{(i + 1) % count};\npub fn f{i}() {{}}\n"),
        "javascript": ("js", lambda i: f"import './file{(i + 1) % count}.js';\n"),
        "typescript": ("ts", lambda i: f"import './file{(i + 1) % count}';\n"),
        "c": ("c", lambda i: f'#include "file{(i + 1) % count}.h"\n'),
        "cpp": ("cpp", lambda i: f'#include "file{(i + 1) % count}.hpp"\n'),
        "csharp": ("cs", lambda i: f"using Bench.File{(i + 1) % count};\n"),
        "go": ("go", lambda i: 'package bench\nimport "example.com/bench/go/pkg"\n'),
        "java": ("java", lambda i: f"package bench.file{i};\nimport bench.file{(i + 1) % count}.File;\n"),
        "python": ("py", lambda i: f"import python.file{(i + 1) % count}\n"),
        "php": ("php", lambda i: f"<?php require './file{(i + 1) % count}.php';\n"),
        "ruby": ("rb", lambda i: f"require_relative './file{(i + 1) % count}'\n"),
        "bash": ("sh", lambda i: f"source ./file{(i + 1) % count}.sh\n"),
        "lua": ("lua", lambda i: f"local x = require('lua.file{(i + 1) % count}')\n"),
        "dart": ("dart", lambda i: f"import 'package:bench_fixture/dart/file{(i + 1) % count}.dart';\n"),
        "swift": ("swift", lambda i: "import Foundation\n"),
        "kotlin": ("kt", lambda i: f"package bench.file{i}\nimport bench.file{(i + 1) % count}.File\n"),
    }
    target = root / "python" / "file0.py"
    for language, (extension, content_for) in languages.items():
        directory = root / language
        directory.mkdir(parents=True, exist_ok=True)
        for index in range(count):
            (directory / f"file{index}.{extension}").write_text(content_for(index))
    return target


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path, help="bpx-rag executable to measure")
    parser.add_argument("--compare", type=Path, help="optional baseline bpx-rag executable")
    parser.add_argument("--runs", type=int, default=25)
    parser.add_argument("--files-per-language", type=int, default=8)
    args = parser.parse_args()
    binary = args.binary.resolve()
    if args.runs < 3 or args.files_per_language < 1:
        parser.error("--runs must be at least 3 and --files-per-language must be positive")

    with tempfile.TemporaryDirectory(prefix="bpx-rag-benchmark-") as temporary:
        root = Path(temporary)
        target = fixture_files(root, args.files_per_language)
        measurements = [("binary", binary)]
        if args.compare:
            measurements.append(("compare", args.compare.resolve()))
        timings_by_binary = {label: [] for label, _ in measurements}
        for iteration in range(args.runs):
            run_order = measurements if iteration % 2 == 0 else list(reversed(measurements))
            for label, executable in run_order:
                start = time.perf_counter()
                subprocess.run(
                    [str(executable), "analyze-file", str(target), "--root", str(root)],
                    check=True,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                )
                timings_by_binary[label].append(time.perf_counter() - start)

    print(f"runs={args.runs} files_per_language={args.files_per_language} languages=17")
    for label, executable in measurements:
        timings = timings_by_binary[label]
        ordered = sorted(timings)
        p95 = ordered[math.ceil(0.95 * len(ordered)) - 1]
        print(f"{label}_path={executable}")
        print(f"{label}_binary_bytes={executable.stat().st_size}")
        print(f"{label}_median_seconds={statistics.median(timings):.6f}")
        print(f"{label}_p95_seconds={p95:.6f}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
