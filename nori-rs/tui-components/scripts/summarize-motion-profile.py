#!/usr/bin/env python3
"""Summarize raw motion_storybook CSVs (plain or gzip), without external packages."""
import argparse
import csv
import gzip
import math
import statistics
from collections import defaultdict
from pathlib import Path


def percentile(values, fraction):
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * fraction) - 1)]


def summarize(rows):
    budget = 1_000_000 / float(rows[0]['fps'])
    result = {'frames': len(rows)}
    for key in ['widget_us', 'output_us', 'reset_us', 'total_us', 'diff_probe_us',
                'changed_cells', 'ansi_bytes', 'write_calls', 'interval_us']:
        values = [float(row[key]) for row in rows if row[key]]
        if values:
            result.update({f'{key}_mean': statistics.mean(values),
                           f'{key}_p50': percentile(values, .50),
                           f'{key}_p95': percentile(values, .95),
                           f'{key}_p99': percentile(values, .99),
                           f'{key}_max': max(values)})
    result['work_over_budget_pct'] = sum(float(row['total_us']) > budget for row in rows) * 100 / len(rows)
    if rows[0]['mode'] == 'live':
        intervals = [float(row['interval_us']) for row in rows]
        result['produced_fps'] = 1_000_000 / statistics.mean(intervals)
        result['intervals_over_1_5_budget_pct'] = sum(t > budget * 1.5 for t in intervals) * 100 / len(intervals)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('inputs', nargs='+', type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    groups, phases = defaultdict(list), defaultdict(list)
    for path in args.inputs:
        opener = gzip.open if path.suffix == '.gz' else open
        with opener(path, 'rt') as stream:
            rows = list(csv.DictReader(stream))
        for row in rows:
            # Exclude the first full repaint/startup frame of every measured run.
            if int(row['frame']) == 0:
                continue
            key = tuple(row[k] for k in ['mode', 'build', 'width', 'height', 'variant', 'fps'])
            groups[key].append(row)
            phase = float(row['phase'])
            band = 'early_0-.45' if phase < .45 else 'middle_.45-.75' if phase < .75 else 'late_.75-1'
            phases[key + (band,)].append(row)
    args.output.mkdir(parents=True, exist_ok=True)
    for name, data in [('summary', groups), ('phases', phases)]:
        records = []
        for key, rows in sorted(data.items()):
            record = dict(zip(['mode', 'build', 'width', 'height', 'variant', 'fps', 'phase_band'], key))
            record.update(summarize(rows))
            records.append(record)
        columns = list(dict.fromkeys(k for record in records for k in record))
        with (args.output / f'{name}.csv').open('w') as stream:
            writer = csv.DictWriter(stream, columns)
            writer.writeheader()
            writer.writerows(records)
        print(args.output / f'{name}.csv')


if __name__ == '__main__':
    main()
