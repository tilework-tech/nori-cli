"""Compare existing release examples sequentially, without terminal I/O or rebuilds."""
import csv, hashlib, json, math, os, pathlib, random, statistics, subprocess, time
HERE = pathlib.Path(__file__).resolve().parent
CURRENT = pathlib.Path('/home/clifford/Documents/source/nori/cli/nori-rs/target/release/examples/motion_storybook')
OTHER = pathlib.Path('/home/clifford/.codex/worktrees/b08b/nori/cli/.worktrees/animated-onboarding/nori-rs/target/release/examples/motion_profile')
env = dict(os.environ, TERM='xterm-256color', COLORTERM='truecolor')
env.pop('NO_COLOR', None)
cases = [(impl, size, variant, rep) for impl in ['current', 'worktree'] for size in ['120x40', '240x80'] for variant in ['default', 'muster'] for rep in range(2)]
random.Random(910).shuffle(cases)
metadata = {'binaries': {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in [CURRENT, OTHER]}, 'runs': [], 'notes': 'Existing release builds; different visuals and benchmark harnesses; 150 frames per process at simulated 25 FPS. First frame excluded. No terminal I/O. Two repetitions; sequential shuffled order.'}
groups = {}
for impl, size, variant, rep in cases:
    path = HERE / f'{impl}-{size}-{variant}-{rep}.csv'
    if impl == 'current':
        command = [str(CURRENT), '--bench', '--size', size, '--frames', '150', '--profile', str(path)]
        if variant == 'muster': command.append('--muster')
    else:
        width, height = size.split('x')
        command = [str(OTHER), '--width', width, '--height', height, '--frames', '150', '--repeats', '1', '--offset', '94' if variant == 'muster' else '0', '--output', str(path)]
    start = time.monotonic()
    subprocess.run(command, env=env, check=True, capture_output=True, timeout=45)
    metadata['runs'].append({'command': command, 'wall_seconds': time.monotonic()-start})
    rows = list(csv.DictReader(path.open()))[1:]
    groups.setdefault((impl,size,variant), []).extend(rows)
    print(f'{impl} {size} {variant} repeat {rep} done', flush=True)
(HERE/'metadata.json').write_text(json.dumps(metadata, indent=2)+'\n')
with (HERE/'summary.csv').open('w') as out:
    writer = csv.writer(out)
    writer.writerow(['implementation','size','variant','frames','median_total_ms','p95_total_ms','p95_widget_ms','over_40ms_pct','mean_bytes'])
    for (impl,size,variant), rows in sorted(groups.items()):
        total = sorted(float(r['total_us'])/1000 for r in rows)
        widget = sorted(float(r['widget_us' if impl == 'current' else 'render_us'])/1000 for r in rows)
        p95 = lambda seq: seq[math.ceil(len(seq)*.95)-1]
        writer.writerow([impl,size,variant,len(rows),statistics.median(total),p95(total),p95(widget),100*sum(v>40 for v in total)/len(total),statistics.mean(float(r['ansi_bytes' if impl == 'current' else 'bytes']) for r in rows)])
print((HERE/'summary.csv').read_text())
