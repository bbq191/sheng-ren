#!/usr/bin/env python3
"""怪癖测试书：每个探针一对章节（A 对照、B 只多一条要测的 CSS），阅读器排出来以后逐行比 A、B 的位置和大小，看这条 CSS 认不认、差多少。

用法:
  probe.py make 输出.epub                 生成测试书（探针见 PROBES；书名「测试-怪癖」）
  probe.py measure 排出来的.pdf [--json]  量 xochitl 排出的 PDF（每页按标记 Q??A/Q??B 认）
  probe.py move [--host=root@10.11.99.1]  生成 → 经书架服务导入 Move → 等它排好 → 拉回 PDF → measure（测试书留在 Move 上，用户自己删）
  （都可加 --only=88-99 或 --only=13,28：只跑这几个探针；--optimize=target/release/epub-optimize：先按那台的模式优化，看修过以后的样子）
  probe.py ireader                        生成（标记换成条码图）→ adb 推进掌阅 documents/测试/ → 文件管理器里点开 → 逐页截屏、认条码、量
                                          （测试书留在掌阅上，用户自己删；掌阅要开着 adb、亮屏解锁）

掌阅没有排好的 PDF，只能逐页截屏：每章第一行的标记换成一张条码图（两端各一格黑，中间 7 位探针序号、1 位 A/B，宽 50%），截屏后
按格取样认出是哪一页；翻页在页底空白处横向滑（碰到字会弹查词）。

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
    ("75", "p margin-left:-2em", "sheet", "margin-left:-2em", P),
    ("76", "div 背景色、两侧 margin -2em（对照两侧 0）", "markup", "", '<div class="bgd">{text}</div>|<div class="bgd nm">{text}</div>'),
    ("77", "h3 背景色、两侧 margin -2em（对照两侧 0）", "markup", "", '<h3 class="bgd">{text}</h3>|<h3 class="bgd nm">{text}</h3>'),
    ("78", "div 背景色、margin-left:-30px（对照 0）", "markup", "", '<div class="bgd">{text}</div>|<div class="bgd nl30">{text}</div>'),
    ("79", "div 背景色、margin-right:-2em（对照 0）", "markup", "", '<div class="bgd">{text}</div>|<div class="bgd nr">{text}</div>'),
    ("80", "p line-height:2", "sheet", "line-height:2", P),
    ("81", "div line-height:2", "sheet", "line-height:2", '<div{cls}>{text}</div>'),
    ("82", "h3 line-height:2", "sheet", "line-height:2", '<h3{cls}>{text}</h3>'),
    ("83", "p 行内 style line-height:2", "inline", "line-height:2", P),
    ("84", "p line-height:3em", "sheet", "line-height:3em", P),
    ("85", "span text-decoration:underline !important（行内）", "markup", "", '<p><span>{text}</span></p>|<p><span style="text-decoration:underline !important">{text}</span></p>'),
    ("86", "p text-align:center !important", "sheet", "text-align:center !important", P),
    ("87", "span text-decoration:underline（行内，对照没有）", "markup", "", '<p><span>{text}</span></p>|<p><span style="text-decoration:underline">{text}</span></p>'),
    # 行内元素、图、列表项、单元格的内外边距（2026-10-13）：短行，Δ右 就是挪了多少
    ("88", "span margin-left:2em（短行，看 Δ右）", "markup", "", '<p>甲<span>乙乙</span></p>|<p>甲<span class="sml2">乙乙</span></p>'),
    ("89", "span margin-left:30px（短行）", "markup", "", '<p>甲<span>乙乙</span></p>|<p>甲<span class="sml30">乙乙</span></p>'),
    ("90", "span padding-left:2em（短行）", "markup", "", '<p>甲<span>乙乙</span></p>|<p>甲<span class="spl2">乙乙</span></p>'),
    ("91", "span padding-left:30px（短行）", "markup", "", '<p>甲<span>乙乙</span></p>|<p>甲<span class="spl30">乙乙</span></p>'),
    ("92", "span 行内 style margin-left:30px（短行）", "markup", "", '<p>甲<span>乙乙</span></p>|<p>甲<span style="margin-left:30px">乙乙</span></p>'),
    ("93", "行内图 margin-left:2em（短行）", "markup", "", '<p>甲<img class="ri" src="rule.png" alt=""/>乙</p>|<p>甲<img class="ri im2" src="rule.png" alt=""/>乙</p>'),
    ("94", "行内图 margin-left:30px（短行）", "markup", "", '<p>甲<img class="ri" src="rule.png" alt=""/>乙</p>|<p>甲<img class="ri im30" src="rule.png" alt=""/>乙</p>'),
    ("95", "li padding-top:2em（第二项）", "markup", "", '<ul><li>{text}</li><li>{text}</li></ul>|<ul><li>{text}</li><li class="lpt2">{text}</li></ul>'),
    ("96", "li margin-top:2em（第二项）", "markup", "", '<ul><li>{text}</li><li>{text}</li></ul>|<ul><li>{text}</li><li class="lmt2">{text}</li></ul>'),
    ("97", "td padding-left:2em（类规则）", "markup", "", '<table><tbody><tr><td>{text}</td></tr></tbody></table>|<table><tbody><tr><td class="tpl2">{text}</td></tr></tbody></table>'),
    ("98", "td 行内 padding-left:30px", "markup", "", '<table><tbody><tr><td>{text}</td></tr></tbody></table>|<table><tbody><tr><td style="padding-left:30px">{text}</td></tr></tbody></table>'),
    ("99", "td 行内 padding-top:30px", "markup", "", '<table><tbody><tr><td>{text}</td></tr></tbody></table>|<table><tbody><tr><td style="padding-top:30px">{text}</td></tr></tbody></table>'),
]

# 测试书全书共用的几条（"markup" 探针用）
BASE_CSS = [".mb0{margin-bottom:0;}", ".mt0{margin-top:0;}", ".mt001{margin-top:0.01em;}", ".ml2{margin-left:2em;}", ".mt2{margin-top:2em;}", ".mb2{margin-bottom:2em;}", ".mt1{margin-top:1em;}", "img.rule{width:100%;}", "img.ruleb{display:block;width:100%;}", ".bgd{background-color:#444444;color:#ffffff;margin-left:0;margin-right:0;}", ".nm{margin-left:-2em;margin-right:-2em;}", ".nl30{margin-left:-30px;}", ".nr{margin-right:-2em;}",
            ".sml2{margin-left:2em;}", ".sml30{margin-left:30px;}", ".spl2{padding-left:2em;}", ".spl30{padding-left:30px;}", "img.ri{width:3em;height:0.5em;}", ".im2{margin-left:2em;}", ".im30{margin-left:30px;}",
            ".lpt2{padding-top:2em;}", ".lmt2{margin-top:2em;}", ".tpl2{padding-left:2em;}"]


def page(code, cid, body, head_style="", barcode=None):
    style = f"<style>{head_style}</style>" if head_style else ""
    mark = f'<img class="code" src="{barcode}" alt=""/>' if barcode else code
    return f'''<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="zh" xml:lang="zh">
<head><title>{code}</title><link rel="stylesheet" type="text/css" href="s.css"/>{style}</head>
<body><p id="{cid}">{mark}</p><p>{BEFORE}</p>{body}<p>{AFTER}</p></body></html>'''


CODE_CELLS = 10  # 条码格数：黑、7 位序号、A/B、黑


def code_png(n, b):
    """第 n 个探针（从 1 起）的条码：10 格、每格 10×10 像素，黑 = 1。"""
    import struct
    import zlib
    bits = [1] + [(n >> (6 - i)) & 1 for i in range(7)] + [int(b), 1]
    row = b"\x00" + b"".join((b"\x00" if bit else b"\xff") * 10 for bit in bits)
    raw = row * 10
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 10 * len(bits), 10, 8, 0, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


def decode_code(png):
    """截屏里第一行（条码）认出 (探针序号, 是不是 B)；认不出 None。"""
    from PIL import Image
    ls = [l for l in lines_of(png) if l[0] < 1600]
    if not ls:
        return None
    top, bottom, left, right = ls[0]
    im = Image.open(png).convert("L")
    y = (top + bottom) // 2
    w = (right - left + 1) / CODE_CELLS
    bits = [im.getpixel((int(left + (i + 0.5) * w), y)) < 128 for i in range(CODE_CELLS)]
    if not (bits[0] and bits[-1]) or bottom - top < 10:
        return None
    n = sum(1 << (6 - i) for i in range(7) if bits[1 + i])
    return n, bits[8]


def chapters(barcode=False):
    """[(章节 id, 标记, xhtml)]，样式表规则。`barcode`：标记换成条码图（掌阅，[`code_png`]）。"""
    out, rules = [], list(BASE_CSS) + ["img.code{width:50%;}"]
    for n, (code, _desc, via, css, tpl) in enumerate(PROBES, 1):
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
        ca, cb = (f"code-{n}-a.png", f"code-{n}-b.png") if barcode else (None, None)
        out.append((f"q{code}a", f"Q{code}A", page(f"Q{code}A", f"q{code}a", a_tpl.format(cls="", text=TEXT), "", ca)))
        out.append((f"q{code}b", f"Q{code}B", page(f"Q{code}B", f"q{code}b", b_tpl.format(cls=cls_b, text=TEXT), head, cb)))
    # 带条码图的每章一张图、字又少，优化器会当成漫画（≥20 张图、平均每张不到 40 字）：末尾加一章长文字（不是探针，放最后不影响认页）
    if barcode:
        out.append(("filler", "filler", page("filler", "filler", "<p>" + "补字" * 1000 + "</p>")))
    return out, rules


OPTIMIZE = None  # --optimize=<epub-optimize>：测试书先按这台的模式优化再推（看修过以后的样子）


def make(path, title="测试-怪癖", barcode=False, device=None):
    if OPTIMIZE and device:
        raw = path + ".raw.epub"
        make(raw, title, barcode)
        subprocess.run([OPTIMIZE, f"--device={device}", raw, path], check=True, capture_output=True)
        return
    chs, rules = chapters(barcode)
    css = "\n".join(rules) + "\n"
    bid = "urn:uuid:" + str(uuid.uuid4())
    nav_items = "".join(f'<li><a href="{c}.xhtml#{c}">{m}</a></li>' for c, m, _ in chs)
    nav = f'''<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="zh" xml:lang="zh">
<head><title>目录</title></head><body><nav epub:type="toc"><ol>{nav_items}</ol></nav></body></html>'''
    manifest = "".join(f'<item id="{c}" href="{c}.xhtml" media-type="application/xhtml+xml"/>' for c, _, _ in chs)
    code_files = [(f"code-{n}-{ab}.png", code_png(n, ab == "b")) for n in range(1, len(PROBES) + 1) for ab in "ab"] if barcode else []
    codes = "".join(f'<item id="{f[:-4]}" href="{f}" media-type="image/png"/>' for f, _ in code_files)
    spine = "".join(f'<itemref idref="{c}"/>' for c, _, _ in chs)
    opf = f'''<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="bid" xml:lang="zh">
<metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="bid">{bid}</dc:identifier><dc:title>{title}</dc:title><dc:creator>测试</dc:creator><dc:language>zh</dc:language><meta property="dcterms:modified">2026-10-13T00:00:00Z</meta></metadata>
<manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="css" href="s.css" media-type="text/css"/><item id="rule" href="rule.png" media-type="image/png"/>{codes}{manifest}</manifest>
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
        for f, data in code_files:
            z.writestr(f"OEBPS/{f}", data, compress_type=zipfile.ZIP_STORED)


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
    make(book, device="xochitl")
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


def adb(*a, out=None):
    r = subprocess.run(["adb", *a], capture_output=True)
    if out:
        with open(out, "wb") as f:
            f.write(r.stdout)
    return r.stdout.decode("utf-8", "replace")


def tap_text(text):
    """掌阅当前界面上含 `text` 的控件，点它的中心；没有返回 False。"""
    import re
    adb("shell", "uiautomator", "dump", "/sdcard/ui.xml")
    xml = adb("shell", "cat", "/sdcard/ui.xml")
    m = re.search(r'text="[^"]*' + re.escape(text) + r'[^"]*"[^>]*bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"', xml)
    if not m:
        return False
    x1, y1, x2, y2 = map(int, m.groups())
    adb("shell", "input", "tap", str((x1 + x2) // 2), str((y1 + y2) // 2))
    time.sleep(2)
    return True


def ireader_run():
    """推进掌阅、打开、逐页截屏量。返回同 [`measure`] 的 {编号: {"A": 行, "B": 行}}。"""
    work = tempfile.mkdtemp(prefix="quirks-ir-")
    # 每次换个文件名：同名的书掌阅记着上次读到哪，会从那里接着翻
    name = f"quirks-ir-{time.strftime('%m%d%H%M%S')}"
    book = os.path.join(work, f"{name}.epub")
    make(book, "测试-怪癖-掌阅", barcode=True, device="ireader")
    adb("shell", "mkdir", "-p", "/sdcard/documents/测试")
    adb("push", book, f"/sdcard/documents/测试/{name}.epub")
    adb("shell", "am", "start", "-n", "com.android.documentsui/com.szzy.ireader.ink.fileexplorer.ui.activity.MainActivity")
    time.sleep(3)
    # 文件管理器停在「测试」里时列表不刷新：点面包屑回 documents 再进来；在哪一层都试几次
    for _ in range(6):
        if tap_text(name):
            break
        if not tap_text("测试") and not tap_text("documents"):
            tap_text("存储空间")
        time.sleep(2)
    else:
        sys.exit(f"文件管理器里没找到「{name}」")
    time.sleep(8)
    codes = {i: c for i, (c, *_) in enumerate(PROBES, 1)}
    out, seen, misses = {}, set(), 0
    for k in range(2 * len(PROBES) + 20):
        png = os.path.join(work, f"p{k}.png")
        adb("exec-out", "screencap", "-p", out=png)
        got = decode_code(png)
        if got is None:
            misses += 1
            if misses > 5:
                break
        elif got in seen:
            # 翻页没翻过去（滑到了字上弹出菜单之类）：退出菜单再滑一次，连着几次就是到头了
            adb("shell", "input", "keyevent", "KEYCODE_BACK")
            misses += 1
            if misses > 5:
                break
        else:
            misses = 0
            seen.add(got)
            n, b = got
            if n in codes:
                out.setdefault(codes[n], {})["B" if b else "A"] = [l for l in lines_of(png) if l[0] < 1600]
            if len(seen) == 2 * len(PROBES):
                break
        adb("shell", "input", "swipe", "1100", "1580", "150", "1580", "200")
        time.sleep(2)
    print(f"掌阅：认出 {len(seen)} 页（共 {2 * len(PROBES)}）；截屏在 {work}；测试书 documents/测试/{name}.epub 用完请删", file=sys.stderr)
    return out


def main():
    global PROBES, OPTIMIZE
    args, opts = cli_args(1, __doc__), cli_opts()
    cmd = args[0]
    # --only=88-99 或 --only=13,28：只跑这几个探针（掌阅全跑一遍要 10 分钟）
    if "--only" in opts:
        want = set()
        for part in opts["--only"].split(","):
            lo, _, hi = part.partition("-")
            want.update(range(int(lo), int(hi or lo) + 1))
        PROBES = [p for p in PROBES if int(p[0]) in want]
    OPTIMIZE = opts.get("--optimize")
    if cmd == "make" and len(args) == 2:
        make(args[1])
    elif cmd in ("measure", "move", "ireader") and (cmd != "measure" or len(args) == 2):
        if cmd == "ireader":
            m = ireader_run()
        else:
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
