#!/usr/bin/env python3
"""真书回归比较（配合 run.sh）。

用法: compare.py <旧目录> <新目录> [--strip-old-markers]

对每本书报一行：
  SAME       zip 里每个条目逐字节相同（纯重构应该是这个：EPUB 产物是确定的）
  TEXT-SAME  有条目变了，但 spine 顺序拼起来的可见文字（去标签、还原字符引用、去空白、U+FEFF 和控制字符）完全一致
  TEXT-DIFF  可见文字变了：打印第一个不同处，需要逐条说明原因
  MISSING    旧的有、新的没有（按原书路径配对，两边都有 index.txt 时；否则按编号）
  NEW        新的有、旧的没有（两次回归之间增删了书）：只做下面两项核对
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
    names = set(z.namelist())
    for idref in re.findall(r'<itemref\b[^>]*\bidref=["\']([^"\']+)', opf):
        if idref not in items:
            continue
        p = os.path.normpath(os.path.join(base, unquote(html.unescape(items[idref])))).replace('\\', '/')
        if p not in names:  # spine 指向不存在的条目（原书就坏）：跳过，不让整轮崩
            continue
        t = z.read(p).decode('utf-8', 'replace')
        if strip_markers:
            t = MARKER.sub('', t)
        t = re.sub(r'(?is)<head\b.*?</head>|<script\b.*?</script>|<style\b.*?</style>', '', t)
        t = re.sub(r'(?s)<!--.*?-->', '', t)
        t = re.sub(r'(?s)<[^>]+>', '', t)
        # 去空白、U+FEFF 和 C0 控制字符（XML 1.0 不允许，规范整理会删掉；不是可见文字）
        out.append(re.sub(r'[\s\x00-\x1f]+', '', html.unescape(t)).replace('﻿', ''))
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

    def index(d):
        """编号 → 原书路径（没有 index.txt 时空）。"""
        p = os.path.join(d, 'index.txt')
        if not os.path.exists(p):
            return {}
        return dict(line.rstrip('\n').split('\t', 1) for line in open(p, encoding='utf-8') if '\t' in line)

    def epubs(d):
        return sorted(f[:-5] for f in os.listdir(d) if f.endswith('.epub'))

    io_, in_ = index(old), index(new)
    # 配对：两边都有 index.txt 就按原书路径配（两次回归之间增删、改名了书，编号会错位），否则按编号
    if io_ and in_:
        by_src = {v: k for k, v in in_.items()}
        pairs = [(n, by_src.get(io_.get(n))) for n in epubs(old)]
        paired = {b for _, b in pairs if b}
        pairs += [(None, n) for n in epubs(new) if n not in paired]
    else:
        have = set(epubs(new))
        pairs = [(n, n if n in have else None) for n in epubs(old)] + [(None, n) for n in epubs(new) if n not in set(epubs(old))]
    bad = 0
    inv_old = inv_new = 0
    for na, nb in pairs:
        if nb is None:
            print('MISSING', na, io_.get(na, ''))
            bad += 1
            continue
        zb = zipfile.ZipFile(os.path.join(new, nb + '.epub'))
        za = zipfile.ZipFile(os.path.join(old, na + '.epub')) if na else None
        xo, xn = (invalid_xml(za) if za else 0), invalid_xml(zb)
        inv_old += xo
        inv_new += xn
        tb = spine_text(zb)
        notes = []
        src = in_.get(nb)
        if src and os.path.exists(src):
            ts = spine_text(zipfile.ZipFile(src))
            if collections.Counter(ts) != collections.Counter(tb):
                d_more = collections.Counter(tb) - collections.Counter(ts)
                d_less = collections.Counter(ts) - collections.Counter(tb)
                notes.append(f'对原书字符账不平：多 {dict(d_more.most_common(8))} 少 {dict(d_less.most_common(8))}')
        else:
            # 没核对就不能算通过（计为问题）
            notes.append(f'未核对：原书不在（{src}）' if src else '未核对：原书不在（index.txt 里没有这本的原书路径）')
        if za is None:
            status = 'NEW'
        elif {n: za.read(n) for n in za.namelist()} == {n: zb.read(n) for n in zb.namelist()}:
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
        name = nb if na in (None, nb) else f'{na}→{nb}'
        print(status, name, f'不合法 XML {xo}→{xn}', ('\n   ' + '\n   '.join(notes)) if notes else '')
    if inv_new > inv_old:
        bad += 1
    print(f'有问题的书: {bad}；不合法 XML 合计 {inv_old} → {inv_new}')
    sys.exit(1 if bad else 0)


main()
