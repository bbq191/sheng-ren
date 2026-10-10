"""按阅读顺序打印第 N 个版面的节点树：python3 tree.py 书.kfx N"""
import sys
from kfx import by_type, cli_args, pools, reading_order, short

kfx_path, want = cli_args(2, "用法：python3 tree.py 书.kfx 版面序号")[:2]
E=by_type(kfx_path)
pl=pools(E)
def text(ref): return pl[ref['name']][ref['$403']]
def node(n,d=0,out=print):
    n=dict(n); kids=n.pop('$146',None); tref=n.pop('$145',None)
    s='  '*d+short({k:v for k,v in n.items() if k not in ('$696',)},maxlen=40)
    if tref is not None:
        s+='  «'+(text(tref) if isinstance(tref,dict) else str(tref))[:60]+'»'
    if '$696' in n: s+=f'  [$696 len {len(n["$696"])}]'
    out(s)
    for k in kids or []: node(k,d+1,out)
order=reading_order(E)
want=int(want)
sec=E['$260'][order[want]]
print('section',order[want],short(sec,maxlen=40))
for pt in sec['$141']:
    sl=E['$259'][pt['$176']]
    for n in sl['$146']: node(n,1)
