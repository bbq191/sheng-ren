"""EPUB ↔ KFX 对照：按文字把 KFX 的文字节点配回 EPUB 元素，汇总 (标签+类) → KFX 样式/类型。
用法：python3 pair.py 书.kfx 原书.epub [--unmatched]
KFX 一侧走 `kfx-dump --json`（kfx.text_nodes）。EPUB 一侧要的是每块文字所在的标签和 class，`epub-to-kfx --styles` 只给算好的样式、
不给标签，所以这里仍按 spine 顺序自己切块（只认块级标签，不算样式）。"""
import os, sys, zipfile, re, collections
from html.parser import HTMLParser
from kfx import by_type, cli_args, short, text_nodes

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'regress'))
from regresslib import spine_paths  # noqa: E402  读 container.xml、href 还原字符引用和百分号解码

kfx_path, epub_path = cli_args(2, "用法：python3 pair.py 书.kfx 原书.epub [--unmatched]")[:2]
E=by_type(kfx_path)
styles=E.get('$157',{})
def norm(s): return re.sub(r'\s+','',s)

# KFX 一侧：storyline 怎么走由 kfx.text_nodes 统一（s2kcmp、tree 同一份）；每个节点记 (类型, 样式名)
knodes=[(norm(t), [(n.get('$159'), n.get('$157')) for n in chain]) for t,chain in text_nodes(E)]

z=zipfile.ZipFile(epub_path)
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
names=set(z.namelist())
for href in spine_paths(z):
    if href not in names: continue
    p=P(); p.feed(z.read(href).decode('utf-8','replace')); p.flush()
    eblocks+=p.out

# 顺序配对
queue=collections.defaultdict(collections.deque)
for txt,st in eblocks: queue[txt].append(st)
pairs=collections.Counter(); unmatched=0
examples={}
for txt,path in knodes:
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
