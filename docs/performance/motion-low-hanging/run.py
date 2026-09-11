"""Bounded, sequential baseline/candidate runs; all artifacts persist after each case."""
import argparse, csv, hashlib, json, os, pathlib, random, shlex, subprocess, time
p=argparse.ArgumentParser()
p.add_argument('--trace',action='store_true'); p.add_argument('binary',type=pathlib.Path); p.add_argument('label'); p.add_argument('--live',action='store_true'); p.add_argument('--repeats',type=int,default=3); p.add_argument('--sizes',help='Comma-separated WIDTHxHEIGHT list')
a=p.parse_args(); root=pathlib.Path(__file__).resolve().parent/a.label
root.mkdir(exist_ok=True); binary=a.binary.resolve()
env=dict(os.environ,TERM='xterm-256color',COLORTERM='truecolor'); env.pop('NO_COLOR',None)
mode='live' if a.live else 'bench'
sizes=['120x40','240x80'] if a.live else ['120x40','240x80','320x100']
if a.sizes: sizes=a.sizes.split(',')
cases=[(size,muster,rep) for size in sizes for muster in [False,True] for rep in range(a.repeats)]
random.Random(911).shuffle(cases)
metadata={'binary':str(binary),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'mode':mode,'runs':[]}
if (root/f'{mode}-metadata.json').exists(): raise RuntimeError('choose a new label; existing metadata is immutable')
(root/f'{mode}-metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
scripts=pathlib.Path(os.environ.get('TUI_PUPPETEERING_DIR','/home/clifford/.nori/profiles/public/clifford/skills/tui-puppeteering-with-tmux'))
for size,muster,rep in cases:
    variant='muster' if muster else 'platoon'; output=root/f'{mode}-{size}-{variant}-{rep}.csv'
    if output.exists() or output.with_suffix('.csv.gz').exists(): raise RuntimeError(f'refusing to overwrite {output}')
    command=[str(binary),'--frames','150','--profile',str(output)]
    if muster: command+=['--muster']
    if a.trace:
        assert a.live, 'tracing requires a live terminal'
        command=['strace','-c','-e','trace=write,writev','-o',str(output.with_suffix('.strace.txt')),*command]
    start=time.monotonic()
    if not a.live:
        command+=['--bench','--size',size]
        subprocess.run(command,env=env,check=True,capture_output=True,timeout=60)
    else:
        session=f'motion-perf-{os.getpid()}'
        # Wait for input so the size is set before initialization and frame zero.
        launcher=root/'live-launch.sh'
        launcher.write_text('#!/bin/sh\nread -r ready\n'+shlex.join(['env','-u','NO_COLOR','TERM=xterm-256color','COLORTERM=truecolor']+command)+'\nmotion_exit=$?\nprintf "\\nMOTION_DONE_%s\\n" "$motion_exit"\nread -r done\n')
        def tool(name,*args): return subprocess.run([str(scripts/name),*args],check=True,capture_output=True,text=True,timeout=45)
        try:
            tool('tui-start',session,shlex.join(['sh',str(launcher)]))
            width,height=size.split('x'); tool('tmux-isolated','resize-window','-t',session,'-x',width,'-y',height)
            tool('tui-send',session,'--keys','Enter')
            tool('tui-assert',session,'·','5')
            tool('tui-assert',session,'MOTION_DONE_0','35')
        finally:
            tool('tui-stop',session)
            launcher.unlink(missing_ok=True)
    rows=list(csv.DictReader(output.open())); assert len(rows)==150,(output,len(rows))
    assert all(f"{r['width']}x{r['height']}"==size for r in rows)
    metadata['runs'].append({'command':command,'size':size,'muster':muster,'repeat':rep,'wall_seconds':time.monotonic()-start})
    (root/f'{mode}-metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
    print(f'{a.label} {mode} {size} {variant} repeat {rep}: saved',flush=True)
