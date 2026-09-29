#!/usr/bin/env python3
"""真书回归比较（配合 run.sh）。

用法: compare.py <旧目录> <新目录> [--strip-old-markers]

对每本书报一行：
  SAME       zip 里每个条目逐字节相同（纯重构应该是这个：EPUB 产物是确定的）
  TEXT-SAME  有条目变了，但 spine 顺序拼起来的可见文字（去标签、还原字符引用、去空白和 U+FEFF）完全一致
  TEXT-DIFF  可见文字变了：打印第一个不同处，需要逐条说明原因
另外两项核对，任何一本不过都算失败：
  - 新产物里 XHTML/OPF/NCX 不是合法 XML 的数量（不能比旧的多）；
  - 对原书的"字符账"：新产物可见文字的字符计数，和 index.txt 里记的原书 spine 可见文字逐字符相等
    （顺序不管：注释会从原处挪到章末，但一个字不多一个字不少）。

--strip-old-markers：旧产物里优化器自己加的注释标号 `<a href="#…">[N]</a>` 不算正文（v27 起不再加），比较前从旧的里去掉。
退出码：全部通过 0，否则 1。
"""
import collections
import html
import os
import re
import sys
import zipfile
import xml.dom.minidom as md
from urllib.parse import unquote

MARKER = re.compile(r'<a\b[^>]*\bhref=["\']#[^"\']*["\'][^>]*>\s*\[\d+\]\s*</a>')


def spine_text(z, strip_markers=False):
    opf_path = re.search(r'full-path=["\']([^"\']+)', z.read('META-INF/container.xml').decode()).group(1)
    opf = z.read(opf_path).decode('utf-8', 'replace')
    base = os.path.dirname(opf_path)
    items = {}
    for m in re.finditer(r'<item\b[^>]*>', opf):
        t = m.group(0)
        i = re.search(r'\bid=["\']([^"\']+)', t)
        h = re.search(r'\bhref=["\']([^"\']+)', t)
        if i and h:
            items[i.group(1)] = h.group(1)
    out = []
    for idref in re.findall(r'<itemref\b[^>]*\bidref=["\']([^"\']+)', opf):
        p = os.path.normpath(os.path.join(base, unquote(items[idref]))).replace('\\', '/')
        t = z.read(p).decode('utf-8', 'replace')
        if strip_markers:
            t = MARKER.sub('', t)
        t = re.sub(r'(?s)<head.*?</head>', '', t)
        t = re.sub(r'(?s)<!--.*?-->', '', t)
        t = re.sub(r'(?s)<[^>]+>', '', t)
        out.append(re.sub(r'\s+', '', html.unescape(t)).replace('﻿', ''))
    return ''.join(out)


def invalid_xml(z):
    n = 0
    for name in z.namelist():
        if name.endswith(('.xhtml', '.html', '.htm', '.opf', '.ncx')):
            try:
                md.parseString(z.read(name))
            except Exception:
                n += 1
    return n


def main():
    args = [a for a in sys.argv[1:] if not a.startswith('--')]
    strip = '--strip-old-markers' in sys.argv
    if len(args) != 2:
        sys.exit(__doc__)
    old, new = args
    sources = {}
    idx = os.path.join(new, 'index.txt')
    if os.path.exists(idx):
        for line in open(idx, encoding='utf-8'):
            n, _, path = line.rstrip('\n').partition('\t')
            sources[n] = path
    bad = 0
    inv_old = inv_new = 0
    for f in sorted(os.listdir(old)):
        if not f.endswith('.epub'):
            continue
        a, b = os.path.join(old, f), os.path.join(new, f)
        if not os.path.exists(b):
            print('MISSING', f)
            bad += 1
            continue
        za, zb = zipfile.ZipFile(a), zipfile.ZipFile(b)
        io_, in_ = invalid_xml(za), invalid_xml(zb)
        inv_old += io_
        inv_new += in_
        ea = {n: za.read(n) for n in za.namelist()}
        eb = {n: zb.read(n) for n in zb.namelist()}
        tb = spine_text(zb)
        notes = []
        src = sources.get(f[:-5])
        if src and os.path.exists(src):
            ts = spine_text(zipfile.ZipFile(src))
            if collections.Counter(ts) != collections.Counter(tb):
                d_more = collections.Counter(tb) - collections.Counter(ts)
                d_less = collections.Counter(ts) - collections.Counter(tb)
                notes.append(f'对原书字符账不平：多 {dict(d_more.most_common(8))} 少 {dict(d_less.most_common(8))}')
        if ea == eb:
            status = 'SAME'
        else:
            ta = spine_text(za, strip)
            if ta == tb:
                status = 'TEXT-SAME'
            else:
                status = 'TEXT-DIFF'
                k = next((i for i in range(min(len(ta), len(tb))) if ta[i] != tb[i]), min(len(ta), len(tb)))
                notes.append(f'长度 {len(ta)} → {len(tb)}，第一个不同处 @{k}: {ta[max(0, k - 20):k + 30]!r} => {tb[max(0, k - 20):k + 30]!r}')
        if status == 'TEXT-DIFF' or notes:
            bad += 1
        print(status, f, f'不合法 XML {io_}→{in_}', ('\n   ' + '\n   '.join(notes)) if notes else '')
    if inv_new > inv_old:
        bad += 1
    print(f'有问题的书: {bad}；不合法 XML 合计 {inv_old} → {inv_new}')
    sys.exit(1 if bad else 0)


main()
