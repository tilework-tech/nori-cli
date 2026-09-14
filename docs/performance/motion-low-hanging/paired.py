"""Interleave unchanged and optimized binaries to reduce host drift bias."""
import csv, hashlib, json, os, pathlib, random, subprocess, sys
root=pathlib.Path(sys.argv[3]).resolve() if len(sys.argv)>3 else pathlib.Path(__file__).resolve().parent
root.mkdir(parents=True,exist_ok=True)
binaries={'paired-baseline':pathlib.Path(sys.argv[1]).resolve(),'paired-optimized':pathlib.Path(sys.argv[2]).resolve()}
env=dict(os.environ,TERM='xterm-256color',COLORTERM='truecolor');env.pop('NO_COLOR',None)
metadata={name:{'binary':str(binary),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'runs':[]} for name,binary in binaries.items()}
for name in binaries: (root/name).mkdir(exist_ok=True)
cases=[(size,muster,rep) for size in ['120x40','240x80','320x100'] for muster in [False,True] for rep in range(2)]
random.Random(1911).shuffle(cases)
for index,(size,muster,rep) in enumerate(cases):
    order=list(binaries) if index%2==0 else list(reversed(binaries))
    for name in order:
        output=root/name/f"bench-{size}-{'muster' if muster else 'platoon'}-{rep}.csv"
        if output.exists() or output.with_suffix('.csv.gz').exists(): raise RuntimeError(f'refusing to overwrite {output}')
        command=[str(binaries[name]),'--bench','--size',size,'--frames','150','--profile',str(output)]
        if muster: command+=['--muster']
        subprocess.run(command,env=env,check=True,capture_output=True,timeout=60)
        assert len(list(csv.DictReader(output.open())))==150
        metadata[name]['runs'].append(command)
        (root/name/'metadata.json').write_text(json.dumps(metadata[name],indent=2)+'\n')
    print(f'paired {size} {"muster" if muster else "platoon"} repeat {rep}: saved',flush=True)
