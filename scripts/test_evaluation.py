#!/usr/bin/env python3
"""Independent CSV/ledger audit using Python 3.9+ standard-library tools."""
import csv
import io
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / "target/release/payment-routing"
ARGS = [str(BINARY), "--evaluate", "--scenario", "all", "--seeds", "0,1,42",
        "--strategies", "static,reserved,preserve,recompute,tight", "--ticks", "60", "--drain", "60"]


def run(output_format):
    result = subprocess.run(ARGS + ["--format", output_format], check=True,
                            capture_output=True, text=True, timeout=60)
    assert result.stderr == "", result.stderr
    assert "\x1b" not in result.stdout
    return result.stdout


def rows(text):
    result = list(csv.DictReader(io.StringIO(text)))
    assert result and all(None not in row and None not in row.values() for row in result)
    return result


def key(row):
    return row["world"], row["seed"], row["strategy"]


def audit():
    raw = run("csv")
    assert raw == run("csv"), "separate invocations did not reproduce byte-identical CSV"
    results = rows(raw)
    cases = {key(r): r for r in results if r["record"] == "case"}
    payments = defaultdict(list)
    for p in rows(run("payments")):
        payments[key(p)].append(p)
    assert len(cases) == 135 and set(cases) == set(payments)
    workloads = {}
    for k, r in cases.items():
        assert r["status"] == "complete" and r["replay_verified"] == "true"
        assert r["world_config"] and r["strategy_config"]
        ledger = payments[k]
        instructions = [(p["sequence"], p["sender"], p["receiver"], p["amount_cents"],
                         p["arrival"], p["deadline"]) for p in ledger]
        world_seed = k[:2]
        if world_seed in workloads:
            assert instructions == workloads[world_seed], k
        else:
            workloads[world_seed] = instructions
        assert len(ledger) == int(r["generated"]) == int(r["offered"])
        assert sum(int(p["amount_cents"]) for p in ledger) == int(r["generated_volume_cents"])
        assert sum(int(p["actual_fee_cents"]) for p in ledger) == int(r["routing_cost_cents"])
        assert sum(int(p["departed_hops"]) for p in ledger) == int(r["departed_hops"])
        assert sum(p["sla_failed"] == "true" for p in ledger) == int(r["sla_failures"])
        assert sum(p["ever_routed"] == "true" for p in ledger) == int(r["accepted_routes"])
        completed = [p for p in ledger if p["outcome"].startswith("Completed")]
        on_time = [p for p in completed if int(p["terminal_at"]) <= int(p["deadline"])]
        late = [p for p in completed if int(p["terminal_at"]) > int(p["deadline"])]
        assert len(completed) == int(r["completed"])
        assert len(on_time) == int(r["on_time"])
        assert len(late) == int(r["completed_late"])
        assert sum(int(p["amount_cents"]) for p in on_time) == int(r["on_time_volume_cents"])
        assert sum(int(p["terminal_at"]) - int(p["arrival"]) for p in completed) == int(r["completed_elapsed_minutes"])
        for outcome in ["Rejected", "Expired", "Pending"]:
            assert sum(p["outcome"] == outcome for p in ledger) == int(r[outcome.lower()])
        assert int(r["sla_failures"]) == int(r["rejected"]) + int(r["expired"]) + int(r["completed_late"])
        assert sum(p["outcome"] == "Expired" and p["ever_routed"] == "false" for p in ledger) == int(r["never_routed_expired"])
    for a in (r for r in results if r["record"] != "case"):
        selected = [r for r in cases.values() if r["strategy"] == a["strategy"]
                    and (a["record"] == "aggregate" or r["world"] == a["world"])]
        assert len(selected) == int(a["matched_cases"]) == int(a["total_cases"])
        for field in ["generated", "generated_volume_cents", "completed", "on_time", "on_time_volume_cents",
                      "sla_failures", "rejected", "expired", "completed_late", "routing_cost_cents",
                      "completed_elapsed_minutes", "departed_hops", "queue_payment_minutes", "changed_assignments"]:
            assert int(a[field]) == sum(int(r[field]) for r in selected), (a["strategy"], field)
    print("Verified 135 runs, 27 shared workloads, 50 aggregate rows, payment accounting and byte-identical independent replay.")


if __name__ == "__main__":
    audit()
