# 核对 AZW3 与源 EPUB 的可见文字是否逐字符一致：python3 textcheck.py 文件.azw3 源.epub
import sys, os; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
# 用 kf8lib（本地黑盒分析脚本）把 AZW3 的 rawML 按片段索引还原，统计可见文字，与源 EPUB 比较
import sys, re, html, collections, zipfile
from kf8lib import *
f = F(sys.argv[1]); raw = rawml(f)
fd = f.rec(f.u32(0xc0)); flow0_end = struct.unpack('>I', fd[16:20])[0]; flow = raw[:flow0_end].decode('utf-8')
_, frags = parse_indx(f, f.u32(0xf8)); _, skels = parse_indx(f, f.u32(0xfc))
text = collections.Counter()
for (k, v, _) in skels:
    start, ln = v[6][0], v[6][1]
for key, v, _ in frags:
    pass
# 片段内容：每个骨架后面紧跟它的片段
pos = 0; body_chars = collections.Counter()
for (k, v, _), (fk, fv, _) in zip(skels, frags):
    s, l = v[6][0], v[6][1]; flen = fv[6][1]
    frag = raw[s + l: s + l + flen].decode('utf-8')
    t = html.unescape(re.sub(r'(?s)<[^>]*>', '', frag)); body_chars.update(c for c in t if not c.isspace())
z = zipfile.ZipFile(sys.argv[2]); src = collections.Counter()
for n in z.namelist():
    if re.search(r'\.x?html?$', n) and not re.search(r'(^|/)nav[^/]*$', n):
        t = z.read(n).decode('utf-8', 'replace'); m = re.search(r'(?is)<body\b[^>]*>(.*)</body>', t)
        t = html.unescape(re.sub(r'(?s)<[^>]*>', '', m.group(1) if m else '')); src.update(c for c in t if not c.isspace())
ncx = f.u32(0xf4); nn = parse_indx(f, ncx)[1] if ncx != 0xffffffff else []
print('SAME' if body_chars == src else f'DIFF +{sum((body_chars-src).values())} -{sum((src-body_chars).values())} {list((src-body_chars).items())[:6]}', f'frags={len(frags)} toc={len(nn)} size={len(f.d)//1024}KB')
