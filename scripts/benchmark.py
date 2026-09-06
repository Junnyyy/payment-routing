#!/usr/bin/env python3
"""Unix-only fresh-process benchmark supervisor; Python >= 3.9, no packages."""
import argparse
import csv
import hashlib
import json
import os
import platform
from pathlib import Path
import signal
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def supervise(command, timeout, rss_mib):
    """wait4 owns reaping; do not call Popen.poll/wait (would lose rusage)."""
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        start = time.monotonic()
        child = subprocess.Popen(command, cwd=ROOT, stdout=stdout, stderr=stderr)
        reason = None
        last_sample = start
        try:
            while True:
                pid, status, usage = os.wait4(child.pid, os.WNOHANG)
                if pid:
                    break
                now = time.monotonic()
                if now - start >= timeout:
                    reason = "timeout"
                elif now - last_sample >= 0.1:
                    # ps reports KiB on both supported platforms. Sampling can overshoot.
                    sample = subprocess.run(["ps", "-o", "rss=", "-p", str(child.pid)],
                                            capture_output=True, text=True, check=False)
                    if sample.returncode != 0 and sample.stderr.strip():
                        raise RuntimeError("RSS guard unavailable: " + sample.stderr.strip())
                    if sample.stdout.strip() and int(sample.stdout.strip()) > rss_mib * 1024:
                        reason = "rss_limit"
                    last_sample = now
                if reason:
                    try:
                        os.kill(child.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        # The child exited after the poll. Still reap its status
                        # and resources, preserving the already-observed limit.
                        pass
                    _, status, usage = os.wait4(child.pid, 0)
                    break
                time.sleep(0.005)
        except BaseException:
            try:
                os.kill(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            _, status, usage = os.wait4(child.pid, 0)
            child.returncode = os.waitstatus_to_exitcode(status)
            raise
        child.returncode = os.waitstatus_to_exitcode(status)
        elapsed = time.monotonic() - start
        stdout.seek(0)
        lines = stdout.read().decode().splitlines()
        stderr.seek(0)
        errors = stderr.read().decode()
    records = [json.loads(line) for line in lines]
    inputs = next((r for r in records if r.get("kind") == "input"), {})
    result = next((r for r in records if r.get("kind") == "result"), {})
    status = reason or (result.get("status", "error") if child.returncode == 0 else "error")
    return {"status": status, "exit_code": child.returncode, "process_seconds": elapsed,
            "cpu_user_seconds": usage.ru_utime, "cpu_system_seconds": usage.ru_stime,
            "peak_rss_bytes": int(usage.ru_maxrss * (1 if sys.platform == "darwin" else 1024)),
            "minor_faults": usage.ru_minflt, "major_faults": usage.ru_majflt,
            "voluntary_context_switches": usage.ru_nvcsw,
            "involuntary_context_switches": usage.ru_nivcsw,
            "input": inputs, "result": result, "stderr": errors}


def deterministic_result(row):
    # All other result fields, including queue observations and counters, must replay.
    return {k: v for k, v in row["result"].items()
            if k not in ("solve_ns", "instrumented", "tick_ns_p95", "tick_ns_max", "oracle_ns")}


def verify_rows(rows):
    complete = [r for r in rows if r["status"] in ("optimal", "infeasible", "simulated", "feasible", "unresolved")]
    fingerprints = {r["input"]["input_digest"] for r in rows if r["input"]}
    if len(fingerprints) > 1:
        raise AssertionError("input changed between repeats/builds")
    for mode in ("plain", "stats"):
        results = [deterministic_result(r) for r in complete if r["mode"] == mode]
        if results and any(r != results[0] for r in results):
            raise AssertionError("nondeterministic results or counters")
    if len({r["result"]["result_digest"] for r in complete}) > 1:
        raise AssertionError("instrumentation changed the full witness/final state")
    if len({r["status"] for r in complete}) > 1:
        raise AssertionError("builds disagree on feasibility")


def summary(rows):
    timing = [r for r in rows if r["mode"] == "plain"]
    solved = [r for r in timing if r["status"] in ("optimal", "infeasible", "simulated", "feasible", "unresolved")]
    counted = next((r for r in rows if r["mode"] == "stats" and r["result"]), None)
    first = rows[0]
    out = {k: first[k] for k in ("family", "scale", "seed", "ticks")}
    out.update({"repeats": len(timing), "finished": len(solved),
                "timeouts": sum(r["status"] == "timeout" for r in timing),
                "rss_limits": sum(r["status"] == "rss_limit" for r in timing),
                "errors": sum(r["status"] == "error" for r in timing),
                "status": solved[0]["status"] if len(solved) == len(timing) else "censored",
                "counter_status": next((r["status"] for r in rows if r["mode"] == "stats"), "missing")})
    out.update({k: v for k, v in next((r["input"] for r in rows if r["input"]), {}).items()
                if k not in ("kind", "family", "scale", "seed", "ticks")})
    if solved:
        values = [r["result"]["solve_ns"] / 1e6 for r in solved]
        out.update(solve_ms_median=statistics.median(values), solve_ms_min=min(values), solve_ms_max=max(values))
        out.update({k: v for k, v in solved[0]["input"].items()
                    if k not in ("kind", "family", "scale", "seed", "ticks")})
        out.update({k: v for k, v in solved[0]["result"].items()
                    if k not in ("kind", "status", "solve_ns", "instrumented", "rail_metrics")})
        if all("tick_ns_p95" in r["result"] for r in solved):
            out["tick_ns_p95_median"] = statistics.median(r["result"]["tick_ns_p95"] for r in solved)
            out["tick_ns_max_max"] = max(r["result"]["tick_ns_max"] for r in solved)
        if out["status"] == "simulated":
            out["completed_per_sim_minute"] = out["completed"] / out["ticks"]
            out["generated_per_wall_second"] = out["generated"] / (out["solve_ms_median"] / 1000)
            out["ticks_per_wall_second"] = out["ticks"] / (out["solve_ms_median"] / 1000)
            out["queue_mean"] = out["queue_sum"] / out["ticks"]
            out["sla_failure_fraction"] = out["sla_failures"] / out["generated"] if out["generated"] else 0
        elif out.get("payments") and out.get("solve_ms_median"):
            out["instructions_per_wall_second"] = out["payments"] / (out["solve_ms_median"] / 1000)
    for key in ("peak_rss_bytes", "cpu_user_seconds", "cpu_system_seconds", "process_seconds"):
        out[key + "_max"] = max(r[key] for r in timing)
    if counted:
        out.update({k: v for k, v in counted["result"].items() if k in (
            "solver_calls", "path_states", "candidates", "candidate_hops", "assignment_states",
            "complete_assignments", "bound_prunes", "deadline_prunes", "capacity_rejects")})
    return out


def command_output(args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--timeout", type=float, default=3)
    parser.add_argument("--rss-mib", type=int, default=512)
    parser.add_argument("--skip-build", action="store_true")
    args = parser.parse_args()
    if sys.platform not in ("darwin", "linux"):
        parser.error("resource normalization supports macOS and Linux only")
    if args.repeats < 1 or args.timeout <= 0 or args.rss_mib < 32:
        parser.error("positive repeats/timeout and at least 32 MiB required")
    # Fail before creating results if the host sandbox denies the RSS guard.
    subprocess.run(["ps", "-o", "rss=", "-p", str(os.getpid())], check=True, capture_output=True)
    manifest = json.loads(args.manifest.read_text())
    args.output.mkdir(parents=True, exist_ok=False)
    binaries = {}
    for mode in ("plain", "stats"):
        target = ROOT / "target" / "benchmark" / mode
        command = ["cargo", "build", "--locked", "--release", "--example", "benchmark", "--target-dir", str(target)]
        if mode == "stats":
            command += ["--features", "search-stats"]
        if not args.skip_build:
            subprocess.run(command, cwd=ROOT, check=True)
        binaries[mode] = target / "release" / "examples" / "benchmark"
    files = sorted(p for glob in ("src/**/*.rs", "tests/**/*.rs", "benchmarks/**/*.rs", "scripts/*.py", "examples/benchmark.rs", "Cargo.*") for p in ROOT.glob(glob))
    source_hash = hashlib.sha256(b"".join(str(p.relative_to(ROOT)).encode() + b"\0" + p.read_bytes() for p in files)).hexdigest()
    metadata = {"schema": 1, "command": sys.argv, "utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                "platform": platform.platform(), "machine": platform.machine(), "cpu_count": os.cpu_count(),
                "processor": command_output(["sysctl", "-n", "machdep.cpu.brand_string"]) if sys.platform == "darwin" else platform.processor(),
                "rustc": command_output(["rustc", "--version", "--verbose"]), "cargo": command_output(["cargo", "--version"]),
                "python": sys.version, "git_head": command_output(["git", "rev-parse", "HEAD"]),
                "git_status": command_output(["git", "status", "--short"]), "source_sha256": source_hash,
                "lock_sha256": hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest(),
                "binary_sha256": {k: hashlib.sha256(v.read_bytes()).hexdigest() for k, v in binaries.items()},
                "timeout_seconds": args.timeout, "rss_limit_mib": args.rss_mib,
                "rss_sampling_seconds": 0.1, "manifest": manifest}
    (args.output / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    summaries = []
    had_errors = False
    with (args.output / "raw.jsonl").open("w") as log:
        for case in manifest["cases"]:
            family, scale = case["family"], case["scale"]
            seed, ticks = case.get("seed", 42), case.get("ticks", 1000)
            rows = []
            for mode in ("plain", "stats"):
                for repeat in range(args.repeats if mode == "plain" else 1):
                    command = [str(binaries[mode]), family, str(scale), str(seed), str(ticks)]
                    row = supervise(command, args.timeout, args.rss_mib)
                    row.update(family=family, scale=scale, seed=seed, ticks=ticks, mode=mode, repeat=repeat, command=command)
                    log.write(json.dumps(row, sort_keys=True) + "\n")
                    log.flush()
                    rows.append(row)
                    if row["status"] == "error":
                        had_errors = True
            verify_rows(rows)
            record = summary(rows)
            summaries.append(record)
            print(f"{family:28} {scale:5} seed={seed}: {record['status']} {record.get('solve_ms_median', 'NA')} ms", flush=True)
    keys = list(dict.fromkeys(k for row in summaries for k in row))
    with (args.output / "summary.csv").open("w") as output:
        writer = csv.DictWriter(output, fieldnames=keys)
        writer.writeheader()
        writer.writerows(summaries)
    (args.output / "summary.json").write_text(json.dumps(summaries, indent=2) + "\n")
    if had_errors:
        raise SystemExit("Worker errors recorded; inspect raw.jsonl")


if __name__ == "__main__":
    main()
