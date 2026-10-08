"""compare.py、tocchk.py 共用：回归目录的配对、OPF 与 spine 解析。"""
import html
import os
import posixpath
import re
from urllib.parse import unquote


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


def attr(tag, name):
    """标签里某个属性的值（单双引号都认，还原字符引用）；没有时 None。"""
    m = re.search(r'(?:^|\s)' + name + r'\s*=\s*(?:"([^"]*)"|\'([^\']*)\')', tag)
    return None if not m else html.unescape(m.group(1) if m.group(1) is not None else m.group(2))


def opf_path(z):
    c = z.read('META-INF/container.xml').decode('utf-8', 'replace')
    return re.search(r'full-path\s*=\s*["\']([^"\']+)', c).group(1)


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
    """spine 顺序的条目路径（manifest 里找不到的 idref 跳过）。"""
    _, t, items = manifest(z)
    return [items[i][0] for i in (attr(m.group(0), 'idref') for m in re.finditer(r'<(?:\w+:)?itemref\b[^>]*>', t)) if i in items]
