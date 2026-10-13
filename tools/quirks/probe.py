#!/usr/bin/env python3
"""怪癖测试书：每个探针一对章节（A 对照、B 只多一条要测的 CSS），阅读器排出来以后逐行比 A、B 的位置和大小，看这条 CSS 认不认、差多少。

用法:
  probe.py make 输出.epub                 生成测试书（探针见 PROBES；书名「测试-怪癖」）
  probe.py measure 排出来的.pdf [--json]  量 xochitl 排出的 PDF（每页按标记 Q??A/Q??B 认）
  probe.py move [--host=root@10.11.99.1]  生成 → 经书架服务导入 Move → 等它排好 → 拉回 PDF → measure（测试书留在 Move 上，用户自己删）

量法：PDF 按 954×1696 渲染成灰度（同 Move 屏幕像素），每页找出一行一行的字（行投影），记每行上沿、下沿、左右边；
标记那一行之后的行 A、B 逐行相减。Δ上沿 是竖直方向（外边距、内边距、行距），Δ左/Δ右 是水平（缩进、左右边距、对齐），Δ高 是字号。
结果只说明阅读器对这条 CSS 的反应；和 Kindle 比要按 `bookconv::kindle_model` 的换算另算，结论写进 crates/bookconv/data/css-quirks.toml。
"""
import json
import os
import subprocess
import sys
import tempfile
import time
import unicodedata
import urllib.request
import uuid
import zipfile

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from toollib import cli_args, cli_opts  # noqa: E402

TEXT = "测试文字" * 8  # 32 个字：Move 一行约 22 个字，排成两行，看得出首行缩进
BEFORE = "甲段文字甲段文字甲段文字"
AFTER = "乙段文字乙段文字乙段文字"

# (编号, 说明, 写法, 要测的 CSS, 元素模板)：写法 sheet＝外链样式表里 `.t{…}`、inline＝style 属性、style＝<style> 里 `.t{…}`；
# 元素模板里 {cls} 是 A 空、B 写上去的属性，{text} 是要排的字。
P = '<p{cls}>{text}</p>'
PROBES = [
    ("01", "p margin-top:0", "sheet", "margin-top:0", P),
    ("02", "p margin-top:0.01em", "sheet", "margin-top:0.01em", P),
    ("03", "p margin-bottom:0", "sheet", "margin-bottom:0", P),
    ("04", "p margin-top:2em（页中间）", "sheet", "margin-top:2em", P),
    ("05", "h3 margin-top:2em（页中间）", "sheet", "margin-top:2em", '<h3{cls}>{text}</h3>'),
    ("06", "p padding-top:2em（页中间）", "sheet", "padding-top:2em", P),
    ("07", "p border-top:6px solid", "sheet", "border-top:6px solid #000", P),
    ("08", "p margin-top:-1em", "sheet", "margin-top:-1em", P),
    ("09", "p margin-left:2em", "sheet", "margin-left:2em", P),
    ("10", "p margin-left:50px", "sheet", "margin-left:50px", P),
    ("11", "p margin-left:10%", "sheet", "margin-left:10%", P),
    ("12", "p padding-left:2em", "sheet", "padding-left:2em", P),
    ("13", "p text-indent:2em", "sheet", "text-indent:2em", P),
    ("14", "p 悬挂缩进 text-indent:-2em; padding-left:2em", "sheet", "text-indent:-2em;padding-left:2em", P),
    ("15", "p text-indent:10%", "sheet", "text-indent:10%", P),
    ("16", "p text-indent:30px", "sheet", "text-indent:30px", P),
    ("17", "p text-align:right", "sheet", "text-align:right", P),
    ("18", "p text-align:center", "sheet", "text-align:center", P),
    ("19", "p letter-spacing:0.5em", "sheet", "letter-spacing:0.5em", P),
    ("20", "p font-size:1.5em", "sheet", "font-size:1.5em", P),
    ("21", "p font-size:24px", "sheet", "font-size:24px", P),
    ("22", "p font-size:150%", "sheet", "font-size:150%", P),
    ("23", "p font-size:18pt", "sheet", "font-size:18pt", P),
    ("24", "span font-size:1.5em（段落里）", "sheet", "font-size:1.5em", '<p><span{cls}>{text}</span></p>'),
    ("25", "p font-size:large", "sheet", "font-size:large", P),
    ("26", "p 行内 style margin-top:2em", "inline", "margin-top:2em", P),
    ("27", "p 行内 style text-indent:2em", "inline", "text-indent:2em", P),
    ("28", "p margin-left:2em !important", "sheet", "margin-left:2em !important", P),
    ("29", "<style> 里 p margin-left:2em", "style", "margin-left:2em", P),
    ("30", "div width:50%", "sheet", "width:50%", '<div{cls}>{text}</div>'),
    ("31", "li（ul 里，对照是 p）", "markup", "", '<p>{text}</p>|<ul><li>{text}</li></ul>'),
    ("32", "blockquote（对照是 p）", "markup", "", '<p>{text}</p>|<blockquote>{text}</blockquote>'),
    ("33", "div margin-left:2em 套 p", "sheet", "margin-left:2em", '<div{cls}><p>{text}</p></div>'),
    ("34", "div text-indent:2em 套 p（继承）", "sheet", "text-indent:2em", '<div{cls}><p>{text}</p></div>'),
    ("35", "p margin-top:3%", "sheet", "margin-top:3%", P),
    ("36", "p margin-top:30px", "sheet", "margin-top:30px", P),
    ("37", "p margin-top:24pt", "sheet", "margin-top:24pt", P),
    ("38", "p margin:2em 0 0 2em（简写）", "sheet", "margin:2em 0 0 2em", P),
    ("39", "p padding-bottom:2em", "sheet", "padding-bottom:2em", P),
    ("40", "p border-bottom:6px solid", "sheet", "border-bottom:6px solid #000", P),
    ("41", "hr（对照没有）", "markup", "", '<p>{text}</p>|<p>{text}</p><hr/>'),
    ("42", "p 行内 style text-align:center", "inline", "text-align:center", P),
    ("43", "p font-weight:bold（看行高宽不变）", "sheet", "font-weight:bold", P),
    ("44", "p 两条同优先级：先 2em 后 0（后者胜才是 0）", "sheet2", ".t{margin-left:2em;} .t{margin-left:0;}", P),
    ("45", "p 最后一条声明不带分号", "sheet2", ".t{color:#000;margin-left:2em}", P),
    ("46", "p 行内 style margin-left:1em", "inline", "margin-left:1em", P),
    ("47", "p 行内 style margin-left:4em", "inline", "margin-left:4em", P),
    ("48", "p 行内 style margin-left:50px", "inline", "margin-left:50px", P),
    ("49", "p 行内 style font-size:1.5em", "inline", "font-size:1.5em", P),
    ("50", "p 行内 style font-size:24px", "inline", "font-size:24px", P),
    ("51", "<style> 里 p font-size:1.5em", "style", "font-size:1.5em", P),
    ("52", "<style> 里 p text-indent:2em", "style", "text-indent:2em", P),
    ("53", "p margin-left:2rem", "sheet", "margin-left:2rem", P),
    ("54", "p margin-left:25%", "sheet", "margin-left:25%", P),
    ("55", "上一段下边距 0、这段上边距 0（对照没写）", "markup", "", '<p class="mb0">{text}</p><p>{text}</p>|<p class="mb0">{text}</p><p class="mt0">{text}</p>'),
    ("56", "上一段下边距 0、这段上边距 0.01em（对照没写）", "markup", "", '<p class="mb0">{text}</p><p>{text}</p>|<p class="mb0">{text}</p><p class="mt001">{text}</p>'),
    ("57", "类 margin-left:2em、行内 margin-left:0（行内胜才是 0）", "markup", "", '<p class="ml2">{text}</p>|<p class="ml2" style="margin-left:0;">{text}</p>'),
    ("58", "p margin-top:2em 行内 style 对照外链（A 外链 B 行内）", "markup", "", '<p class="mt2">{text}</p>|<p style="margin-top:2em;">{text}</p>'),
    ("59", "p 行内 style padding-left:2em", "inline", "padding-left:2em", P),
    ("60", "p 行内 style text-indent:30px", "inline", "text-indent:30px", P),
    ("61", "p 最后一条不带分号、后面换行再 }", "sheet2", ".t{color:#000;margin-left:2em\n}", P),
    ("62", "p 最后一条不带分号、后面空格再 }", "sheet2", ".t{color:#000;margin-left:2em }", P),
    ("63", "p 只有一条 text-indent:2em、紧挨着 }", "sheet2", ".t{text-indent:2em}", P),
    ("64", "p 只有一条 text-indent:2em、空格再 }", "sheet2", ".t{text-indent:2em }", P),
    ("65", "h3 margin-bottom:2em（页中间）", "sheet", "margin-bottom:2em", '<h3{cls}>{text}</h3>'),
    ("66", "p margin-bottom:2em、后面是 h3", "markup", "", '<p>{text}</p><h3>小标题</h3>|<p class="mb2">{text}</p><h3>小标题</h3>'),
    ("67", "p margin-bottom:2em、后面 h3 margin-top:1em（对照 h3 没写）", "markup", "", '<p class="mb2">{text}</p><h3 class="mt0">小标题</h3>|<p class="mb2">{text}</p><h3 class="mt1">小标题</h3>'),
    ("68", "p background-color:#888", "sheet", "background-color:#888888", P),
    ("69", "段落后面一张 100% 宽的横线图（rule.png 1000×6）", "markup", "", '<p>{text}</p>|<p>{text}</p><img class="rule" src="rule.png" alt=""/>'),
    ("70", "h3 里末尾一张 display:block 的横线图", "markup", "", '<h3>{text}</h3>|<h3>{text}<img class="ruleb" src="rule.png" alt=""/></h3>'),
    ("71", "p margin-left:2em 套着 padding 写法的对照：padding-left 换成 margin-left", "sheet", "margin-left:2em;text-indent:-2em", P),
    ("72", "p margin-bottom:2em（页中间）", "sheet", "margin-bottom:2em", P),
    ("73", "div margin-top:2em（页中间）", "sheet", "margin-top:2em", '<div{cls}>{text}</div>'),
    ("74", "h3 后面 p margin-top:2em（对照 p 没写）", "markup", "", '<h3>小标题</h3><p>{text}</p>|<h3>小标题</h3><p class="mt2">{text}</p>'),
]

# 测试书全书共用的几条（"markup" 探针用）
BASE_CSS = [".mb0{margin-bottom:0;}", ".mt0{margin-top:0;}", ".mt001{margin-top:0.01em;}", ".ml2{margin-left:2em;}", ".mt2{margin-top:2em;}", ".mb2{margin-bottom:2em;}", ".mt1{margin-top:1em;}", "img.rule{width:100%;}", "img.ruleb{display:block;width:100%;}"]


def page(code, cid, body, head_style=""):
    style = f"<style>{head_style}</style>" if head_style else ""
    return f'''<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="zh" xml:lang="zh">
<head><title>{code}</title><link rel="stylesheet" type="text/css" href="s.css"/>{style}</head>
<body><p id="{cid}">{code}</p><p>{BEFORE}</p>{body}<p>{AFTER}</p></body></html>'''


def chapters():
    """[(章节 id, 标记, xhtml)]，样式表规则。"""
    out, rules = [], list(BASE_CSS)
    for code, _desc, via, css, tpl in PROBES:
        a_tpl, _, b_tpl = tpl.partition("|")
        b_tpl = b_tpl or a_tpl
        cls_b, head = "", ""
        # 声明一律带分号：xochitl 把规则里最后一条不带分号的声明丢掉（Q45 专门测这个）
        if via == "sheet":
            rules.append(f".t{code}{{{css};}}")
            cls_b = f' class="t{code}"'
        elif via == "sheet2":
            rules.append(css.replace(".t", f".t{code}"))
            cls_b = f' class="t{code}"'
        elif via == "inline":
            cls_b = f' style="{css}"'
        elif via == "style":
            head = f".t{code}{{{css};}}"
            cls_b = f' class="t{code}"'
        out.append((f"q{code}a", f"Q{code}A", page(f"Q{code}A", f"q{code}a", a_tpl.format(cls="", text=TEXT))))
        out.append((f"q{code}b", f"Q{code}B", page(f"Q{code}B", f"q{code}b", b_tpl.format(cls=cls_b, text=TEXT), head)))
    return out, rules


def make(path, title="测试-怪癖"):
    chs, rules = chapters()
    css = "\n".join(rules) + "\n"
    bid = "urn:uuid:" + str(uuid.uuid4())
    nav_items = "".join(f'<li><a href="{c}.xhtml#{c}">{m}</a></li>' for c, m, _ in chs)
    nav = f'''<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="zh" xml:lang="zh">
<head><title>目录</title></head><body><nav epub:type="toc"><ol>{nav_items}</ol></nav></body></html>'''
    manifest = "".join(f'<item id="{c}" href="{c}.xhtml" media-type="application/xhtml+xml"/>' for c, _, _ in chs)
    spine = "".join(f'<itemref idref="{c}"/>' for c, _, _ in chs)
    opf = f'''<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="bid" xml:lang="zh">
<metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="bid">{bid}</dc:identifier><dc:title>{title}</dc:title><dc:creator>测试</dc:creator><dc:language>zh</dc:language><meta property="dcterms:modified">2026-10-13T00:00:00Z</meta></metadata>
<manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="css" href="s.css" media-type="text/css"/><item id="rule" href="rule.png" media-type="image/png"/>{manifest}</manifest>
<spine>{spine}</spine></package>'''
    container = '<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>'
    with zipfile.ZipFile(path, "w") as z:
        z.writestr(zipfile.ZipInfo("mimetype"), "application/epub+zip", compress_type=zipfile.ZIP_STORED)
        z.writestr("META-INF/container.xml", container, compress_type=zipfile.ZIP_DEFLATED)
        z.writestr("OEBPS/content.opf", opf, compress_type=zipfile.ZIP_DEFLATED)
        z.writestr("OEBPS/nav.xhtml", nav, compress_type=zipfile.ZIP_DEFLATED)
        z.writestr("OEBPS/s.css", css, compress_type=zipfile.ZIP_DEFLATED)
        for c, _, x in chs:
            z.writestr(f"OEBPS/{c}.xhtml", x, compress_type=zipfile.ZIP_DEFLATED)
        z.writestr("OEBPS/rule.png", rule_png(), compress_type=zipfile.ZIP_STORED)


def rule_png(w=1000, h=6):
    """一条黑色横线（PNG，灰度）：试图片能不能当边框画。"""
    import struct
    import zlib
    raw = b"".join(b"\x00" + b"\x00" * w for _ in range(h))
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 0, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


def lines_of(png):
    """一页里一行一行的字：[(上, 下, 左, 右)]（屏幕像素）。"""
    from PIL import Image
    im = Image.open(png).convert("L")
    w, h = im.size
    px = im.load()
    rows = []
    for y in range(h):
        xs = [x for x in range(w) if px[x, y] < 128]
        rows.append((min(xs), max(xs)) if xs else None)
    out, cur = [], None
    for y, r in enumerate(rows + [None]):
        if r is not None:
            cur = [y, y, r[0], r[1]] if cur is None else [cur[0], y, min(cur[2], r[0]), max(cur[3], r[1])]
        elif cur is not None:
            # 同一行字的笔画之间偶尔有一两行空：隔得近的并起来
            if out and cur[0] - out[-1][1] <= 3:
                p = out[-1]
                out[-1] = (p[0], cur[1], min(p[2], cur[2]), max(p[3], cur[3]))
            else:
                out.append(tuple(cur))
            cur = None
    return out


def measure(pdf):
    """{编号: {"A": 行, "B": 行}}，行从标记那一行算起。"""
    with tempfile.TemporaryDirectory() as d:
        n = int(subprocess.run(["pdfinfo", pdf], capture_output=True, text=True).stdout.split("Pages:")[1].split()[0])
        pages = {}
        for i in range(1, n + 1):
            t = subprocess.run(["pdftotext", "-f", str(i), "-l", str(i), pdf, "-"], capture_output=True, text=True).stdout
            t = unicodedata.normalize("NFKC", t)
            for line in t.splitlines():
                s = line.strip()
                if len(s) == 4 and s.startswith("Q") and s[3] in "AB" and s[1:3].isdigit():
                    pages.setdefault(s, i)
                    break
        out = {}
        for mark, i in sorted(pages.items()):
            base = os.path.join(d, f"p{i}")
            subprocess.run(["pdftoppm", "-gray", "-png", "-f", str(i), "-l", str(i), "-scale-to-x", "954", "-scale-to-y", "1696", "-singlefile", pdf, base], check=True)
            out.setdefault(mark[1:3], {})[mark[3]] = lines_of(base + ".png")
        return out


def report(m):
    rows = []
    for code, desc, *_ in PROBES:
        ab = m.get(code, {})
        a, b = ab.get("A"), ab.get("B")
        if not a or not b:
            rows.append((code, desc, "没找到页"))
            continue
        diffs = []
        for i in range(max(len(a), len(b))):
            if i >= len(a) or i >= len(b):
                diffs.append(f"行{i}:{'A' if i >= len(b) else 'B'}多一行")
                continue
            (at, ab_, al, ar), (bt, bb, bl, br) = a[i], b[i]
            d = (bt - at, bl - al, br - ar, (bb - bt) - (ab_ - at))
            if any(abs(v) > 1 for v in d):
                diffs.append(f"行{i}:上{d[0]:+d} 左{d[1]:+d} 右{d[2]:+d} 高{d[3]:+d}")
        rows.append((code, desc, "；".join(diffs) or "无差别"))
    return rows


def move_run(host):
    work = tempfile.mkdtemp(prefix="quirks-")
    book = os.path.join(work, "quirks.epub")
    make(book)
    tunnel = subprocess.Popen(["ssh", "-o", "ControlMaster=no", "-o", "ExitOnForwardFailure=yes", "-N", "-L", "8790:127.0.0.1:8790", host])
    try:
        time.sleep(2)
        data = open(book, "rb").read()
        req = urllib.request.Request("http://127.0.0.1:8790/import?name=quirks.epub&folder=", data=data, headers={"Content-Type": "application/epub+zip"}, method="POST")
        v = json.load(urllib.request.urlopen(req, timeout=120))
        while "job" in v and v.get("state") != "done":
            time.sleep(2)
            v = json.load(urllib.request.urlopen(f"http://127.0.0.1:8790/import/jobs/{v['job']}", timeout=30))
            if v.get("state") == "failed":
                sys.exit(f"导入失败：{v.get('message')}")
        uid = v["uuid"]
    finally:
        tunnel.terminate()
    pdf = os.path.join(work, "quirks.pdf")
    src = f"{host}:/home/root/.local/share/remarkable/xochitl/{uid}.pdf"
    for _ in range(60):
        if subprocess.run(["scp", "-q", src, pdf]).returncode == 0:
            break
        time.sleep(5)
    else:
        sys.exit(f"{uid}.pdf 一直没排出来")
    print(f"Move 上的测试书 uuid {uid}（用完请删）；PDF {pdf}", file=sys.stderr)
    return pdf


def main():
    args, opts = cli_args(1, __doc__), cli_opts()
    cmd = args[0]
    if cmd == "make" and len(args) == 2:
        make(args[1])
    elif cmd in ("measure", "move") and (cmd == "move" or len(args) == 2):
        pdf = args[1] if cmd == "measure" else move_run(opts.get("--host", "root@10.11.99.1"))
        m = measure(pdf)
        if "--json" in sys.argv[1:]:
            print(json.dumps(m, ensure_ascii=False))
            return
        for code, desc, r in report(m):
            print(f"Q{code}\t{desc}\t{r}")
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
