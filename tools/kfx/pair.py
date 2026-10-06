"""EPUB ↔ KFX 对照：按文字把 KFX 的文字节点配回 EPUB 元素，汇总 (标签+类) → KFX 样式/类型。
用法：python3 pair.py 书.kfx 原书.epub"""
import sys, zipfile, re, posixpath, collections
from html.parser import HTMLParser
from kfx import load, cli_args
from ion import short

kfx_path, epub_path = cli_args(2, "用法：python3 pair.py 书.kfx 原书.epub [--unmatched]")[:2]
r,ci,ents=load(kfx_path)
E={}
for i,t,b in ents: E.setdefault(t,{})[i]=b[0] if isinstance(b,list) and b else b
pools={k:v['$146'] for k,v in E.get('$145',{}).items()}
styles=E.get('$157',{})
def norm(s): return re.sub(r'\s+','',s)

knodes=[]
def walk(n, path):
    t=n.get('$145')
    here=path+[(n.get('$159'), n.get('$157'))]
    if isinstance(t,dict) and t.get('name') in pools:
        knodes.append((norm(pools[t['name']][t['$403']]), n, here))
    for k in n.get('$146',[]) or []:
        if isinstance(k,dict): walk(k, here)
order=E['$258'][list(E['$258'])[0]]['$169'][0]['$170']
for sname in order:
    for pt in E['$260'][sname]['$141']:
        sl=E['$259'].get(pt.get('$176'))
        if not sl: continue
        for n in sl.get('$146',[]): walk(n,[])

z=zipfile.ZipFile(epub_path)
opf=[n for n in z.namelist() if n.endswith('.opf')][0]
o=z.read(opf).decode()
man={m.group(1):m.group(2) for m in re.finditer(r'<item[^>]*?id="([^"]+)"[^>]*?href="([^"]+)"',o)}
man.update({m.group(2):m.group(1) for m in re.finditer(r'<item[^>]*?href="([^"]+)"[^>]*?id="([^"]+)"',o)})
spine=[m.group(1) for m in re.finditer(r'<itemref[^>]*idref="([^"]+)"',o)]
BLOCK={'p','h1','h2','h3','h4','h5','h6','div','li','td','th','dt','dd','blockquote','pre','caption','figcaption'}
class P(HTMLParser):
    def __init__(s): super().__init__(); s.stack=[]; s.out=[]; s.buf=None
    def handle_starttag(s,tag,a):
        a=dict(a); s.stack.append((tag,a.get('class',''),a.get('style','')))
        if tag in BLOCK: s.flush(); s.buf=[list(s.stack),[]]
    def handle_startendtag(s,tag,a): pass
    def handle_endtag(s,tag):
        if tag in BLOCK: s.flush()
        while s.stack:
            t=s.stack.pop()[0]
            if t==tag: break
    def handle_data(s,d):
        if s.buf is not None: s.buf[1].append(d)
    def flush(s):
        if s.buf and norm(''.join(s.buf[1])): s.out.append((norm(''.join(s.buf[1])), s.buf[0]))
        s.buf=None if not s.stack else [list(s.stack),[]]
eblocks=[]
base=posixpath.dirname(opf)
for idref in spine:
    href=posixpath.normpath(posixpath.join(base,man[idref]))
    p=P(); p.feed(z.read(href).decode('utf-8','replace')); p.flush()
    eblocks+=p.out

# 顺序配对
queue=collections.defaultdict(collections.deque)
for txt,st in eblocks: queue[txt].append(st)
pairs=collections.Counter(); unmatched=0
examples={}
for txt,n,path in knodes:
    if queue[txt]:
        st=queue[txt].popleft()
        key=' > '.join(f"{t}.{c}".rstrip('.') for t,c,_ in st if t not in ('html',))
        kk=' > '.join(f"{ty}:{sty}" for ty,sty in path)
        pairs[(key,kk)]+=1; examples.setdefault((key,kk),txt[:20])
    else:
        unmatched+=1
        if '--unmatched' in sys.argv: print('没配上：', txt[:60], ' '.join(f'{ty}:{sty}' for ty,sty in path))
print(f'KFX 文字节点 {len(knodes)}，EPUB 块 {len(eblocks)}，没配上 {unmatched}')
for (k,kk),c in pairs.most_common():
    print(f'{c:5d}  {k}\n       → {kk}   «{examples[(k,kk)]}»')
print('\n样式：')
for k,v in styles.items(): print(' ',k,short(v))
