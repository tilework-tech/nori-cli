"""Summarize each stage separately; never pool baseline and candidate samples."""
import csv, gzip, math, pathlib, statistics
root=pathlib.Path(__file__).resolve().parent
summary=[]; phases=[]
def percentile(values,q):
    values=sorted(values); return values[math.ceil(len(values)*q)-1]
for directory in sorted(root.iterdir()):
    if not directory.is_dir() or directory.name.startswith('trace-'): continue
    groups={}
    for path in sorted(list(directory.glob('*.csv')) + list(directory.glob('*.csv.gz'))):
        for row in list(csv.DictReader(gzip.open(path,'rt') if path.suffix=='.gz' else path.open()))[1:]:
            key=(row['mode'],row['variant'],f"{row['width']}x{row['height']}")
            groups.setdefault(key,[]).append(row)
    for (mode,variant,size),rows in sorted(groups.items()):
        totals=[float(r['total_us'])/1000 for r in rows]
        widget=[float(r['widget_us'])/1000 for r in rows]
        output=[float(r['output_us'])/1000 for r in rows]
        intervals=[float(r['interval_us'])/1000 for r in rows]
        result=dict(stage=directory.name,mode=mode,variant=variant,size=size,frames=len(rows),median_ms=statistics.median(totals),p95_ms=percentile(totals,.95),p99_ms=percentile(totals,.99),max_ms=max(totals),over_40ms_pct=100*sum(x>40 for x in totals)/len(totals),widget_p95_ms=percentile(widget,.95),output_p95_ms=percentile(output,.95),produced_fps=1000/statistics.mean(intervals) if mode=='live' else '',interval_p95_ms=percentile(intervals,.95) if mode=='live' else '',gaps_over_60ms_pct=100*sum(x>60 for x in intervals)/len(rows) if mode=='live' else '')
        summary.append(result)
        for name,low,high in [('early',0,.45),('middle',.45,.75),('late',.75,1.01)]:
            selected=[r for r in rows if low<=float(r['phase'])<high]
            if selected: phases.append(dict(stage=directory.name,mode=mode,variant=variant,size=size,phase=name,frames=len(selected),widget_p95_ms=percentile([float(r['widget_us'])/1000 for r in selected],.95),total_p95_ms=percentile([float(r['total_us'])/1000 for r in selected],.95)))
for name,rows in [('summary.csv',summary),('phases.csv',phases)]:
    if rows:
        with (root/name).open('w') as f:
            writer=csv.DictWriter(f,fieldnames=list(rows[0]),lineterminator="\n");writer.writeheader();writer.writerows(rows)
for row in summary:
    if row['stage'] in ['baseline','optimized'] and row['mode']=='live': print(row)
