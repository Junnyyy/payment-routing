#!/usr/bin/env python3
"""Join identical benchmark cohorts; never assign a finite gap to missing work."""
import json
import math
import statistics
import sys
from pathlib import Path


def compare(rows):
    exact = {(r['family'], r['scale'], r['seed'], r['ticks']): r for r in rows if not r['family'].startswith('bounded-')}
    result = []
    for row in rows:
        if not row['family'].startswith('bounded-') or 'sim-' in row['family']:
            continue
        r = dict(row)
        oracle = exact.get((r['family'][8:], r['scale'], r['seed'], r['ticks']))
        if oracle and oracle['status'] == 'infeasible' and r['status'] == 'feasible':
            raise AssertionError('heuristic feasibility contradicts exact infeasibility')
        known = r.get('known_optimum_fee', r.get('reference_fee_cents'))
        source = 'analytical fee certificate' if known is not None else 'unknown'
        if oracle and oracle['status'] == 'optimal':
            known = oracle['fee_cents']
            source = 'exact optimizer'
            r['oracle_ms'] = oracle['solve_ms_median']
            if r['status'] == 'feasible':
                r['elapsed_delta'] = r['objective_minutes'] - oracle['objective_minutes']
                r['hops_delta'] = r['objective_hops'] - oracle['objective_hops']
                r['full_witness_match'] = r['result_digest'] == oracle['result_digest']
        r['optimum_fee'] = known
        r['optimum_source'] = source
        r['oracle_status'] = oracle['status'] if oracle else 'not run'
        r['gap_percent'] = None
        r['gap_status'] = 'unknown optimum'
        if r['status'] != 'feasible':
            r['gap_status'] = 'no full solution'
        elif known is not None:
            assert r['fee_cents'] >= known, 'feasible fee below certified optimum'
            if known == 0 and r['fee_cents'] > 0:
                r['gap_status'] = 'infinite'
            else:
                r['gap_percent'] = 100 * (r['fee_cents'] - known) / known if known else 0
                r['gap_status'] = 'finite'
        result.append(r)
    return result


def main():
    folder = Path(sys.argv[1])
    rows = compare(json.loads((folder / 'summary.json').read_text()))
    (folder / 'gaps.json').write_text(json.dumps(rows, indent=2) + '\n')
    known = [r for r in rows if r['optimum_fee'] is not None]
    gaps = [r['gap_percent'] if r['gap_percent'] is not None else math.inf for r in known]
    finite = [r['gap_percent'] for r in known if r['gap_percent'] is not None]
    print('known:',len(known),'full:',len(finite),'coverage-inclusive median:',statistics.median(gaps) if gaps else None,'served-only median:',statistics.median(finite) if finite else None)
    for r in sorted(known,key=lambda r: r['gap_percent'] if r['gap_percent'] is not None else math.inf,reverse=True)[:12]:
        print(r['family'],r['scale'],r['seed'],r['gap_status'],r['gap_percent'],r.get('solve_ms_median'))

if __name__ == '__main__':
    main()
