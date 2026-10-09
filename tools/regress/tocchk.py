#!/usr/bin/env python3
"""真书回归的目录核对（配合 run.sh、compare.py）：对比新旧两次回归产物的 spine 文件数和 NCX 目录。

用法: tocchk.py <旧目录> <新目录>

每本文字书一行（漫画跳过）：原书、旧、新的 spine 文件数；新旧目录条数；层级+标签是否逐条一致；只改了目标的条数；
nav 条数；新产物里目标文件或 #锚点不存在的目录条目（坏链接）。
算问题（退出码 1）：新 spine 比原书多（文字书不拆文件）、层级或标签变了、有坏链接。
新 spine 比原书少只注明（旧版本的空白页清理会删掉没有可见内容的文件）；补的封面页（`eink-cover.xhtml`）不计入 spine、另行注明。新旧产物按原书路径配对（同 compare.py）；
只有一边有、原书或产物读不出来的报出来算问题，不让整轮崩。"""
import difflib, os, re, sys, zipfile, posixpath
from urllib.parse import unquote
import xml.etree.ElementTree as ET

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from regresslib import attr, index, manifest, pairs, spine_paths  # noqa: E402


def spine_and_ncx(z):
    _, t, items = manifest(z)
    ncx = nav = None
    for p, tag in items.values():
        if (attr(tag, 'media-type') or '') == 'application/x-dtbncx+xml':
            ncx = p
        if 'nav' in (attr(tag, 'properties') or '').split():
            nav = p
    spine = spine_paths(z)
    return spine, ncx, nav


def strip_ns(tag):
    return tag.split('}', 1)[-1]


def ncx_flat(z, ncx):
    root = ET.fromstring(z.read(ncx))
    out = []
    def walk(el, depth):
        for c in el:
            if strip_ns(c.tag) == 'navPoint':
                lab, src = '', ''  # 没有 <content> 的条目记成空目标（坏链接），别沿用上一条的
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
        if not h:
            bad.append('（没有目标）')
            continue
        p, _, f = h.partition('#')
        path = posixpath.normpath(posixpath.join(d, unquote(p))) if p else base
        if path not in names:
            bad.append(h)
            continue
        if f and unquote(f) not in anchors(z, path):
            bad.append(h)
    return bad


COVER_PAGE = 'eink-cover.xhtml'


def info(path):
    z = zipfile.ZipFile(path)
    names = set(z.namelist())
    spine, ncx, nav = spine_and_ncx(z)
    flat = ncx_flat(z, ncx) if ncx and ncx in names else []
    bad = bad_targets(z, ncx, [s for _, _, s in flat], names) if ncx else []
    navl = nav_links(z, nav) if nav and nav in names else []
    bad += bad_targets(z, nav, navl, names) if nav else []
    # 照 Send to Kindle 补的封面页（掌阅、Move：`wash::prepend_cover_page`）不算拆文件，单独计
    added = sum(1 for s in spine if s.rsplit('/', 1)[-1] == COVER_PAGE)
    return dict(spine=len(spine) - added, cover=added, flat=flat, bad=bad, nav=len(navl), z=z)


def orig_spine(path):
    z = zipfile.ZipFile(path)
    return len(spine_and_ncx(z)[0])


def main():
    if len(sys.argv) != 3 or sys.argv[1] in ('-h', '--help'):
        print(__doc__)
        sys.exit(0 if sys.argv[1:2] in (['-h'], ['--help']) else 2)
    old, new = sys.argv[1], sys.argv[2]
    srcs = index(new)
    print('| # | 书 | 原 spine | 旧 spine | 新 spine | 旧目录条数 | 新目录条数 | 层级+标签一致 | 只改目标 | nav 条数 | 坏链接 |')
    print('|---|---|---|---|---|---|---|---|---|---|---|')
    problems = 0
    for na, n in pairs(old, new):
        if 'comic' in (na, n):
            continue
        if na is None or n is None:
            print(f"| {n or na} | {'只有新的' if na is None else '只有旧的'} |")
            problems += 1
            continue
        src = srcs.get(n, '')
        try:
            o, w = info(f'{old}/{na}.epub'), info(f'{new}/{n}.epub')
            os_ = orig_spine(src)
        except Exception as e:  # 产物或原书读不出来：这本算问题，不让整轮崩
            print(f'| {n} | {src.rsplit("/", 1)[-1][:28]} | 读不出来：{type(e).__name__}: {e} |')
            problems += 1
            continue
        ol = [(d, l) for d, l, _ in o['flat']]
        nl = [(d, l) for d, l, _ in w['flat']]
        same_lv = ol == nl
        tgt_diff = sum(1 for a, b in zip(o['flat'], w['flat']) if a[2] != b[2])
        name = src.rsplit('/', 1)[-1][:28]
        print(f"| {n} | {name} | {os_} | {o['spine']} | {w['spine']} | {len(o['flat'])} | {len(w['flat'])} | {'是' if same_lv else '否'} | {tgt_diff} | {w['nav']} | {len(w['bad'])} |")
        if w['cover']:
            print('     注：补了封面页（原书没有封面页，不算拆文件）')
        if w['spine'] > os_ or len(w['bad']) or not same_lv:
            problems += 1
        elif w['spine'] < os_:
            print(f"     注：比原书少 {os_ - w['spine']} 个 spine 文件（空页清理）")
        if not same_lv:
            for line in list(difflib.unified_diff([f'{d} {l}' for d, l in ol], [f'{d} {l}' for d, l in nl], lineterm='', n=0))[:12]:
                print('    ', line)
        for b in w['bad'][:5]:
            print('     BAD', b)
    print(f'\n有问题的书：{problems}')
    sys.exit(1 if problems else 0)


if __name__ == '__main__':
    main()
