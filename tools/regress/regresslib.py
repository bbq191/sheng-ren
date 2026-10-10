"""compare.py、tocchk.py、kfx/pair.py 共用：回归目录的配对、OPF 与 spine 解析。"""
import os
import posixpath
import re
import sys
from urllib.parse import unquote

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from toollib import attr, opf_path  # noqa: E402,F401  tocchk 从这里拿 attr；OPF 定位和 kfx/ 的脚本共用一份


def index(d):
    """编号 → 原书路径（没有 index.txt 时空）。"""
    p = os.path.join(d, 'index.txt')
    if not os.path.exists(p):
        return {}
    return dict(line.rstrip('\n').split('\t', 1) for line in open(p, encoding='utf-8') if '\t' in line)


def epubs(d):
    return sorted(f[:-5] for f in os.listdir(d) if f.endswith('.epub'))


def pairs(old, new):
    """新旧产物配对 [(旧编号或 None, 新编号或 None)]。两边都有 index.txt 就按原书路径配
    （两次回归之间增删、改名了书，编号会错位），否则按编号。"""
    io_, in_ = index(old), index(new)
    if io_ and in_:
        by_src = {v: k for k, v in in_.items()}
        out = [(n, by_src.get(io_.get(n))) for n in epubs(old)]
        paired = {b for _, b in out if b}
        return out + [(None, n) for n in epubs(new) if n not in paired]
    have, had = set(epubs(new)), set(epubs(old))
    return [(n, n if n in have else None) for n in epubs(old)] + [(None, n) for n in epubs(new) if n not in had]


def manifest(z):
    """(OPF 路径, OPF 文本, {id: (条目路径, 开标签)})；href 还原字符引用、百分号解码、规整路径。"""
    op = opf_path(z)
    d = posixpath.dirname(op)
    t = z.read(op).decode('utf-8', 'replace')
    items = {}
    for m in re.finditer(r'<(?:\w+:)?item\b[^>]*>', t):
        tag = m.group(0)
        i, h = attr(tag, 'id'), attr(tag, 'href')
        if i is not None and h is not None:
            items[i] = (posixpath.normpath(posixpath.join(d, unquote(h))), tag)
    return op, t, items


def spine_paths(z):
    """spine 顺序的条目路径（manifest 里找不到的 idref 跳过）。标了 `linear="no"` 的导航文档（目录页）不算：原书自己说它不在阅读顺序里，
    三台的产物都把它拿出 spine（2026-10-09 用户定；文件还在，阅读器的目录照常），新旧、原书一样不计。"""
    _, t, items = manifest(z)
    out = []
    for m in re.finditer(r'<(?:\w+:)?itemref\b[^>]*>', t):
        i = attr(m.group(0), 'idref')
        if i not in items:
            continue
        nav = 'nav' in (attr(items[i][1], 'properties') or '').split()
        if nav and (attr(m.group(0), 'linear') or '').strip() == 'no':
            continue
        out.append(items[i][0])
    return out
