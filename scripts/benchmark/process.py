#!/usr/bin/env python3
"""Run one fresh bpx-rag process and record elapsed time, I/O, and peak RSS."""

import argparse
import json
import os
import resource
import subprocess
import time
from pathlib import Path


def process_stats(pid):
    root = Path(f"/proc/{pid}")
    try:
        io_values = dict(
            (key.rstrip(":"), int(value))
            for key, value in (line.split() for line in (root / "io").read_text().splitlines())
        )
        status_values = {}
        for line in (root / "status").read_text().splitlines():
            key, _, value = line.partition(":")
            if key in {"VmHWM", "VmRSS"}:
                status_values[key] = int(value.strip().split()[0])
        return io_values.get("read_bytes", 0), status_values.get("VmHWM", 0), status_values.get("VmRSS", 0)
    except (FileNotFoundError, PermissionError, ProcessLookupError, ValueError):
        return None


def warm_files(paths):
    total = 0
    for value in paths:
        path = Path(value)
        files = [path] if path.is_file() else (item for item in path.rglob("*") if item.is_file())
        for file_path in files:
            with file_path.open("rb") as handle:
                while block := handle.read(8 * 1024 * 1024):
                    total += len(block)
    return total


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--label", required=True)
    parser.add_argument("--data-home", required=True)
    parser.add_argument("--config-home", required=True)
    parser.add_argument("--hf-home")
    parser.add_argument("--warm-path", action="append", default=[], help="read this file or tree before starting the measured process")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("provide a command after --")

    args.output_dir.mkdir(parents=True, exist_ok=True)
    paths = {suffix: args.output_dir / f"{args.label}.{suffix}" for suffix in ("stdout", "stderr", "json")}
    if any(path.exists() for path in paths.values()):
        parser.error(f"output already exists for label {args.label}")
    prewarmed_bytes = warm_files(args.warm_path)
    env = os.environ.copy()
    env.update({"XDG_DATA_HOME": args.data_home, "XDG_CONFIG_HOME": args.config_home})
    if args.hf_home:
        env["HF_HOME"] = args.hf_home

    started = time.perf_counter()
    with paths["stdout"].open("wb") as stdout, paths["stderr"].open("wb") as stderr:
        process = subprocess.Popen(command, stdout=stdout, stderr=stderr, env=env)
        read_bytes = peak_hwm_kb = peak_rss_kb = 0
        while process.poll() is None:
            values = process_stats(process.pid)
            if values:
                read_bytes = max(read_bytes, values[0])
                peak_hwm_kb = max(peak_hwm_kb, values[1])
                peak_rss_kb = max(peak_rss_kb, values[2])
            time.sleep(0.02)
        return_code = process.returncode
    elapsed_ms = (time.perf_counter() - started) * 1000
    usage = resource.getrusage(resource.RUSAGE_CHILDREN)
    result = {
        "command": command,
        "return_code": return_code,
        "elapsed_ms": round(elapsed_ms, 3),
        "prewarmed_bytes": prewarmed_bytes,
        "proc_read_bytes": read_bytes if read_bytes else None,
        "filesystem_input_blocks": usage.ru_inblock,
        "peak_hwm_kb": max(peak_hwm_kb, int(usage.ru_maxrss)),
        "peak_rss_kb": peak_rss_kb,
        "stdout": str(paths["stdout"]),
        "stderr": str(paths["stderr"]),
    }
    paths["json"].write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result, indent=2))
    raise SystemExit(return_code)


if __name__ == "__main__":
    main()
