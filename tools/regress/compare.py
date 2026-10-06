#!/usr/bin/env python3
"""真书回归比较（配合 run.sh）。

用法: compare.py <旧目录> <新目录> [--strip-old-markers]

对每本书报一行：
  SAME       zip 里每个条目逐字节相同（纯重构应该是这个：EPUB 产物是确定的）
  TEXT-SAME  有条目变了，但 spine 顺序拼起来的可见文字（去标签、还原字符引用、去空白、U+FEFF 和控制字符）完全一致
  TEXT-DIFF  可见文字变了：打印第一个不同处，需要逐条说明原因
  MISSING    旧的有、新的没有（按原书路径配对，两边都有 index.txt 时；否则按编号）
  NEW        新的有、旧的没有（两次回归之间增删了书）：只做下面两项核对
  BROKEN     产物读不出来（不是 zip、没有 container.xml 等）
另外两项核对，任何一本不过都算失败：
  - 新产物里 XHTML/OPF/NCX 不是合法 XML 的数量（不能比旧的多）；
  - 对原书的"字符账"：新产物可见文字的字符计数，和 index.txt 里记的原书 spine 可见文字逐字符相等
    （顺序不管：注释会从原处挪到章末，但一个字不多一个字不少）；正文图片（<img>、SVG <image>）不比原书少。唯一的例外是只有图标的注释号换成数字（用户定的规则）：
    只多出 ASCII 数字、原书里有只含图片的链接、多出的位数不超过按图标个数连续编号的位数时，只注明、不算不平。

--strip-old-markers：旧产物里优化器自己加的注释标号 `<a href="#…">[N]</a>` 不算正文（v27 起不再加），比较前从旧的里去掉。
退出码：全部通过 0，否则 1。
"""
import collections
import html
import os
import re
import sys
import zipfile
import xml.parsers.expat
from urllib.parse import unquote

MARKER = re.compile(r'<a\b[^>]*\bhref=["\']#[^"\']*["\'][^>]*>\s*\[\d+\]\s*</a>')


def spine_paths(z):
    """spine 顺序的条目路径（manifest 里找不到的 idref 跳过）。"""
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
    return [os.path.normpath(os.path.join(base, unquote(html.unescape(items[i])))).replace('\\', '/')
            for i in re.findall(r'<itemref\b[^>]*\bidref=["\']([^"\']+)', opf) if i in items]


def spine_text(z, strip_markers=False):
    out = []
    names = set(z.namelist())
    for p in spine_paths(z):
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


LINK = re.compile(r'(?is)<a\b[^>]*\bhref=["\'][^"\']*#[^>]*>(.*?)</a>')


def body_images(z):
    """spine 里正文图片的个数（`<img>`、SVG `<image>`），不算只有图、没有文字的链接里的图（图标注释号，优化器换成数字）。"""
    n = 0
    names = set(z.namelist())
    for p in spine_paths(z):
        if p not in names:
            continue
        t = re.sub(r'(?is)<head\b.*?</head>', '', z.read(p).decode('utf-8', 'replace'))
        n += len(re.findall(r'(?i)<(?:img|image)\b', t))
        for inner in LINK.findall(t):
            if not re.sub(r'(?s)<[^>]+>|\s', '', inner):
                n -= len(re.findall(r'(?i)<(?:img|image)\b', inner))
    return n


def icon_note_links(src):
    """原书里只有图、没有文字的链接个数（图标注释号；优化器会把它换成数字）。"""
    n = 0
    with zipfile.ZipFile(src) as z:
        for name in z.namelist():
            if re.search(r'\.x?html?$', name, re.I):
                for inner in LINK.findall(z.read(name).decode('utf-8', 'replace')):
                    if re.search(r'(?i)<img\b', inner) and not re.sub(r'(?s)<[^>]+>|\s', '', inner):
                        n += 1
    return n


def icon_digit_budget(n):
    """n 个注释号最多多出的数字个数：每章从 1 编号也不会超过 1..n 连写的位数。"""
    return sum(len(str(i)) for i in range(1, n + 1))


def invalid_xml(z):
    n = 0
    for name in z.namelist():
        if name.endswith(('.xhtml', '.html', '.htm', '.opf', '.ncx')):
            # 直接用 expat（minidom 底下也是它）：带命名空间处理，未声明的前缀照样算不合法；不建 DOM，大书快得多
            try:
                xml.parsers.expat.ParserCreate(namespace_separator=' ').Parse(z.read(name), True)
            except xml.parsers.expat.ExpatError:
                n += 1
    return n


def same_entries(za, zb):
    """两个 zip 的条目（名字和字节）完全相同。先比目录里的名字、大小、CRC，都对上了再逐个条目比字节（不整本读进内存）。"""
    ia = sorted((i.filename, i.file_size, i.CRC) for i in za.infolist())
    ib = sorted((i.filename, i.file_size, i.CRC) for i in zb.infolist())
    return ia == ib and all(za.read(n) == zb.read(n) for n, _, _ in ia)


def main():
    args = [a for a in sys.argv[1:] if not a.startswith('--')]
    flags = [a for a in sys.argv[1:] if a.startswith('--')]
    strip = '--strip-old-markers' in flags
    if len(args) != 2 or any(f != '--strip-old-markers' for f in flags):
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
        try:
            zb = zipfile.ZipFile(os.path.join(new, nb + '.epub'))
            za = zipfile.ZipFile(os.path.join(old, na + '.epub')) if na else None
            xo, xn = (invalid_xml(za) if za else 0), invalid_xml(zb)
            tb = spine_text(zb)
        except Exception as e:  # 产物坏了（不是 zip、没有 container.xml 等）：这本报出来，不让整轮崩
            print('BROKEN', nb, f'{type(e).__name__}: {e}')
            bad += 1
            continue
        inv_old += xo
        inv_new += xn
        notes = []
        info = []  # 只说明、不算问题
        src = in_.get(nb)
        ts = None
        if src and os.path.exists(src):
            try:
                with zipfile.ZipFile(src) as zs:
                    ts = spine_text(zs)
            except Exception as e:
                notes.append(f'未核对：原书读不出来（{type(e).__name__}: {e}）')
        if ts is not None:
            # 图片账：正文图片一张不能少（字符账只管文字，只有图的页被删了它看不出来）
            try:
                with zipfile.ZipFile(src) as zs:
                    ia = body_images(zs)
                ib = body_images(zb)
                if ib < ia:
                    notes.append(f'对原书图片账不平：原书正文图 {ia} 张，新产物 {ib} 张')
            except Exception as e:
                notes.append(f'图片未核对（{type(e).__name__}: {e}）')
            if collections.Counter(ts) != collections.Counter(tb):
                d_more = collections.Counter(tb) - collections.Counter(ts)
                d_less = collections.Counter(ts) - collections.Counter(tb)
                # 只有图标的注释标号换成了数字（用户定的规则）：只多出 ASCII 数字、且原书有这种图标链接时不算不平，只报个数
                icons = icon_note_links(src) if not d_less and set(d_more) <= set('0123456789') else 0
                if icons and sum(d_more.values()) <= icon_digit_budget(icons):
                    info.append(f'注：多出 {sum(d_more.values())} 个数字（原书 {icons} 个图标注释号换成了数字，不算不平）')
                else:
                    notes.append(f'对原书字符账不平：多 {dict(d_more.most_common(8))} 少 {dict(d_less.most_common(8))}')
        elif not notes:
            # 没核对就不能算通过（计为问题）
            notes.append(f'未核对：原书不在（{src}）' if src else '未核对：原书不在（index.txt 里没有这本的原书路径）')
        if za is None:
            status = 'NEW'
        elif same_entries(za, zb):
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
        print(status, name, f'不合法 XML {xo}→{xn}', ''.join('\n   ' + x for x in notes + info))
    if inv_new > inv_old:
        bad += 1
    print(f'有问题的书: {bad}；不合法 XML 合计 {inv_old} → {inv_new}')
    sys.exit(1 if bad else 0)


main()
