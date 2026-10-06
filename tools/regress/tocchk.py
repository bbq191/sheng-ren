#!/usr/bin/env python3
"""真书回归的目录核对（配合 run.sh、compare.py）：对比新旧两次回归产物的 spine 文件数和 NCX 目录。

用法: tocchk.py <旧目录> <新目录>

每本文字书一行（漫画跳过）：原书、旧、新的 spine 文件数；新旧目录条数；层级+标签是否逐条一致；只改了目标的条数；
nav 条数；新产物里目标文件或 #锚点不存在的目录条目（坏链接）。
算问题（退出码 1）：新 spine 比原书多（文字书不拆文件）、层级或标签变了、有坏链接。
新 spine 比原书少只注明（章尾空白页清理会删掉没有可见内容的文件）。"""
import re, sys, zipfile, posixpath, html
from urllib.parse import unquote
import xml.etree.ElementTree as ET


def opf_path(z):
    c = z.read('META-INF/container.xml').decode('utf-8', 'replace')
    return re.search(r'full-path\s*=\s*["\']([^"\']+)', c).group(1)


def attr(tag, name):
    m = re.search(r'(?:^|\s)' + name + r'\s*=\s*(?:"([^"]*)"|\'([^\']*)\')', tag)
    return None if not m else html.unescape(m.group(1) if m.group(1) is not None else m.group(2))


def spine_and_ncx(z):
    op = opf_path(z)
    d = posixpath.dirname(op)
    t = z.read(op).decode('utf-8', 'replace')
    items = {}
    ncx = nav = None
    for m in re.finditer(r'<(?:\w+:)?item\b[^>]*>', t):
        tag = m.group(0)
        i, h = attr(tag, 'id'), attr(tag, 'href')
        if i is None or h is None:
            continue
        p = posixpath.normpath(posixpath.join(d, unquote(h)))
        items[i] = p
        if (attr(tag, 'media-type') or '') == 'application/x-dtbncx+xml':
            ncx = p
        if 'nav' in (attr(tag, 'properties') or '').split():
            nav = p
    spine = [items.get(attr(m.group(0), 'idref')) for m in re.finditer(r'<(?:\w+:)?itemref\b[^>]*>', t)]
    return [s for s in spine if s], ncx, nav


def strip_ns(tag):
    return tag.split('}', 1)[-1]


def ncx_flat(z, ncx):
    root = ET.fromstring(z.read(ncx))
    out = []
    def walk(el, depth):
        for c in el:
            if strip_ns(c.tag) == 'navPoint':
                label = ''.join(x.text or '' for x in c.iter() if strip_ns(x.tag) == 'text')
                lab = ''
                for x in c:
                    if strip_ns(x.tag) == 'navLabel':
                        lab = ''.join((y.text or '') for y in x.iter() if strip_ns(y.tag) == 'text')
                    if strip_ns(x.tag) == 'content':
                        src = x.get('src')
                out.append((depth, lab.strip(), src))
                walk(c, depth + 1)
    for c in root:
        if strip_ns(c.tag) == 'navMap':
            walk(c, 1)
    return out


def nav_links(z, nav):
    root = ET.fromstring(z.read(nav))
    out = []
    for n in root.iter():
        if strip_ns(n.tag) == 'nav' and any(v == 'toc' for k, v in n.attrib.items() if k.endswith('type')):
            for a in n.iter():
                if strip_ns(a.tag) == 'a' and a.get('href'):
                    out.append(a.get('href'))
    return out


_anchor_cache = {}


def anchors(z, path):
    key = (z.filename, path)  # 别用 id(z)：前一本的 ZipFile 回收后 id 会被下一本复用，拿到别的书的锚点
    if key not in _anchor_cache:
        t = z.read(path).decode('utf-8', 'replace')
        ids = set()
        for m in re.finditer(r'<(\w[\w:-]*)\b[^>]*>', t):
            tag = m.group(0)
            v = attr(tag, 'id')
            if v:
                ids.add(v)
            if m.group(1).lower() == 'a':
                v = attr(tag, 'name')
                if v:
                    ids.add(v)
        _anchor_cache[key] = ids
    return _anchor_cache[key]


def bad_targets(z, base, hrefs, names):
    bad = []
    d = posixpath.dirname(base)
    for h in hrefs:
        p, _, f = h.partition('#')
        path = posixpath.normpath(posixpath.join(d, unquote(p))) if p else base
        if path not in names:
            bad.append(h)
            continue
        if f and unquote(f) not in anchors(z, path):
            bad.append(h)
    return bad


def info(path):
    z = zipfile.ZipFile(path)
    names = set(z.namelist())
    spine, ncx, nav = spine_and_ncx(z)
    flat = ncx_flat(z, ncx) if ncx and ncx in names else []
    bad = bad_targets(z, ncx, [s for _, _, s in flat], names) if ncx else []
    navl = nav_links(z, nav) if nav and nav in names else []
    bad += bad_targets(z, nav, navl, names) if nav else []
    return dict(spine=len(spine), flat=flat, bad=bad, nav=len(navl), z=z)


def orig_spine(path):
    z = zipfile.ZipFile(path)
    return len(spine_and_ncx(z)[0])


def main():
    if len(sys.argv) != 3 or sys.argv[1] in ('-h', '--help'):
        print(__doc__)
        sys.exit(0 if sys.argv[1:2] in (['-h'], ['--help']) else 2)
    old, new = sys.argv[1], sys.argv[2]
    idx = [l.rstrip('\n').split('\t') for l in open(f'{new}/index.txt', encoding='utf-8') if l.strip()]
    print('| # | 书 | 原 spine | 旧 spine | 新 spine | 旧目录条数 | 新目录条数 | 层级+标签一致 | 只改目标 | nav 条数 | 坏链接 |')
    print('|---|---|---|---|---|---|---|---|---|---|---|')
    problems = 0
    for n, src in idx:
        if n == 'comic':
            continue
        o, w = info(f'{old}/{n}.epub'), info(f'{new}/{n}.epub')
        os_ = orig_spine(src)
        same_lv = [(d, l) for d, l, _ in o['flat']] == [(d, l) for d, l, _ in w['flat']]
        tgt_diff = sum(1 for a, b in zip(o['flat'], w['flat']) if a[2] != b[2])
        name = src.rsplit('/', 1)[-1][:28]
        print(f"| {n} | {name} | {os_} | {o['spine']} | {w['spine']} | {len(o['flat'])} | {len(w['flat'])} | {'是' if same_lv else '否'} | {tgt_diff} | {w['nav']} | {len(w['bad'])} |")
        if w['spine'] > os_ or len(w['bad']) or not same_lv:
            problems += 1
        elif w['spine'] < os_:
            print(f"     注：比原书少 {os_ - w['spine']} 个 spine 文件（空页清理）")
        if not same_lv:
            ol = [(d, l) for d, l, _ in o['flat']]
            nl = [(d, l) for d, l, _ in w['flat']]
            import difflib
            for line in list(difflib.unified_diff([f'{d} {l}' for d, l in ol], [f'{d} {l}' for d, l in nl], lineterm='', n=0))[:12]:
                print('    ', line)
        for b in w['bad'][:5]:
            print('     BAD', b)
    print(f'\n有问题的书：{problems}')
    sys.exit(1 if problems else 0)


if __name__ == '__main__':
    main()
