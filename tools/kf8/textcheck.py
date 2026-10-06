# 核对 AZW3 与源 EPUB 的可见文字是否逐字符一致：python3 textcheck.py 文件.azw3 源.epub
# AZW3 一侧用 kf8lib（本地黑盒分析脚本）按骨架/片段索引取出各片段；EPUB 一侧按 OPF spine 顺序取各 XHTML 的 body。
# 两边都去掉标签、注释、script/style、还原字符引用、丢掉空白，再把整本书的字符序列按顺序逐字比对（不只是比字符计数）。
import sys, os; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import re, html, posixpath, zipfile
from urllib.parse import unquote
from kf8lib import F, rawml, parse_indx, cli_args

def visible(fragment):
    t = re.sub(r'(?is)<!--.*?-->|<script\b.*?</script>|<style\b.*?</style>', '', fragment)
    t = html.unescape(re.sub(r'(?s)<[^>]*>', '', t))
    return ''.join(c for c in t if not c.isspace())

def attr(tag, name):
    m = re.search(r'(?is)\s' + name + r'\s*=\s*(?:"([^"]*)"|\'([^\']*)\')', tag)
    return html.unescape(m.group(1) if m.group(1) is not None else m.group(2)) if m else None

def azw3_text(path):
    f = F(path); raw = rawml(f)
    _, frags = parse_indx(f, f.u32(0xf8)); _, skels = parse_indx(f, f.u32(0xfc))
    ncx = f.u32(0xf4); toc = len(parse_indx(f, ncx)[1]) if ncx != 0xffffffff else 0
    # 每个骨架后面紧跟它的片段（写出器的布局），片段序号即 spine 顺序
    parts = []
    for (_, v, _), (_, fv, _) in zip(skels, frags):
        s, l = v[6][0], v[6][1]
        parts.append(visible(raw[s + l: s + l + fv[6][1]].decode('utf-8')))
    return parts, dict(frags=len(frags), toc=toc, size=len(f.d) // 1024)

def epub_text(path):
    z = zipfile.ZipFile(path)
    container = z.read('META-INF/container.xml').decode('utf-8', 'replace')
    opf_path = attr(re.search(r'(?is)<rootfile\b[^>]*>', container).group(0), 'full-path')
    opf = z.read(opf_path).decode('utf-8', 'replace'); opf_dir = posixpath.dirname(opf_path)
    items = {}
    for m in re.finditer(r'(?is)<item\b[^>]*>', opf):
        i, h, mt = attr(m.group(0), 'id'), attr(m.group(0), 'href'), attr(m.group(0), 'media-type') or ''
        if i and h:
            items[i] = (posixpath.normpath(posixpath.join(opf_dir, unquote(h))), mt)
    names = set(z.namelist()); parts = []
    for m in re.finditer(r'(?is)<itemref\b[^>]*>', opf):
        p, mt = items.get(attr(m.group(0), 'idref'), (None, ''))
        if p is None or 'html' not in mt or p not in names:
            continue
        t = z.read(p).decode('utf-8', 'replace')
        b = re.search(r'(?is)<body\b[^>]*>(.*)</body>', t)
        parts.append(visible(b.group(1) if b else ''))
    return parts

azw3_path, epub_path = cli_args(2, "用法：python3 textcheck.py 文件.azw3 源.epub")[:2]
a, info = azw3_text(azw3_path); b = epub_text(epub_path)
sa, sb = ''.join(a), ''.join(b)
stat = f"docs={len(a)}/{len(b)} chars={len(sa)} frags={info['frags']} toc={info['toc']} size={info['size']}KB"
if sa == sb and len(a) == len(b):
    print('SAME', stat)
else:
    i = next((k for k in range(min(len(sa), len(sb))) if sa[k] != sb[k]), min(len(sa), len(sb)))
    print(f'DIFF at char {i}: azw3 …{sa[max(0, i-20):i+20]!r}… epub …{sb[max(0, i-20):i+20]!r}…', stat)
    sys.exit(1)
