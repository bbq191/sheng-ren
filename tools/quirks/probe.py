#!/usr/bin/env python3
"""怪癖测试书：每个探针一对章节（A 对照、B 只多一条要测的 CSS），阅读器排出来以后逐行比 A、B 的位置和大小，看这条 CSS 认不认、差多少。

用法:
  probe.py make 输出.epub                 生成测试书（探针见 PROBES；书名「测试-怪癖」）
  （都可加 --only=88-99 或 --only=13,28,100-119：只跑这几个探针；--optimize=target/release/epub-optimize：先按那台的模式优化，看修过以后的样子）
  probe.py ireader [--json] [--max-y=1828] [--min-y=90] 生成（标记换成条码图）→ adb 推进掌阅 documents/测试/ → 文件管理器里点开 → 逐页截屏、认条码、量
                                          （测试书留在掌阅上，维护者自己删；掌阅要开着 adb、亮屏解锁）
  probe.py kobo generate                  Kobo（没有 adb，分几步、截屏要手动）：生成 EPUB 和 KEPUB 两本（每次换文件名），放进 --run= 目录
                                          （缺省 target/quirks-kobo/<名字>/）；--bin=<目录> 指 epub-to-kepub 所在（缺省 target/release）
  probe.py kobo push [--run=目录]          拷进 Kobo 的 <挂载点>/<kobo.toml 的 dir>/测试/（USB 大容量存储，挂载点找法同 booklib）、sync，打印手动步骤
  probe.py kobo collect --format=epub|kepub [--run=目录] [--pattern=screen_*.png] [--clean]
                                          重新插上后把设备根目录的截屏拷回 <run>/shots/<格式>/（拷过的不再拷）再量；--clean 才删设备上拷回来的截屏
  probe.py kobo measure --format=epub|kepub [--run=目录 | --from-dir=目录] [--json]   只量电脑上的截屏（离线）（--from-dir 时 --only 要和生成时一样）
  probe.py kobo report [--run=目录]         EPUB、KEPUB 两份结果并排，供手填 css-quirks.toml 的 kobo 一栏
  （kobo 都可加 --max-y=1376：截屏里只看这个高度以上的行，去掉页脚；缺省按 1448 高的 95%，未上真机；--min-y= 去页眉，缺省 0 不去）

掌阅没有排好的 PDF，只能逐页截屏：每章第一行的标记换成一张条码图（两端各一格黑，中间 8 位探针序号、1 位 A/B，宽 50%），截屏后
按格取样认出是哪一页；翻页在页底空白处横向滑（碰到字会弹查词）。

2026-10-31 以前还有 measure、move（Move 上 xochitl 排出的 PDF），Move 卖了以后删了。

量法：每页截屏转灰度，找出一行一行的字（行投影），记每行上沿、下沿、左右边；
标记那一行之后的行 A、B 逐行相减。Δ上沿 是竖直方向（外边距、内边距、行距），Δ左/Δ右 是水平（缩进、左右边距、对齐），Δ高 是字号。
结果只说明阅读器对这条 CSS 的反应；和 Kindle 比要按 `bookconv::kindle_model` 的换算另算，结论写进 crates/bookconv/data/css-quirks.toml。
"""
import json
import os
import subprocess
import sys
import tempfile
import time
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
    # 体检里「没验证过」的写法（2026-10-14）：竖直对齐、列表符号、块高、水平线、flex、绝对定位、单元格和块的边框
    ("100", "span vertical-align:super（短行）", "markup", "", '<p>甲乙<span>丙丁</span></p>|<p>甲乙<span class="vsup">丙丁</span></p>'),
    ("101", "span vertical-align:sub（短行）", "markup", "", '<p>甲乙<span>丙丁</span></p>|<p>甲乙<span class="vsub">丙丁</span></p>'),
    ("102", "行内竖长图（宽 10%，比一行高） vertical-align:middle（对照没写）", "markup", "", '<p>甲<img class="rt" src="tall.png" alt=""/>乙</p>|<p>甲<img class="rt vmid" src="tall.png" alt=""/>乙</p>'),
    ("103", "行内竖长图（宽 10%，比一行高） vertical-align:top", "markup", "", '<p>甲<img class="rt" src="tall.png" alt=""/>乙</p>|<p>甲<img class="rt vtop" src="tall.png" alt=""/>乙</p>'),
    ("104", "行内竖长图（宽 10%，比一行高） vertical-align:-0.5em", "markup", "", '<p>甲<img class="rt" src="tall.png" alt=""/>乙</p>|<p>甲<img class="rt vneg" src="tall.png" alt=""/>乙</p>'),
    ("105", "td vertical-align:top（左格两行、右格一行，对照没写）", "markup", "", '<table><tbody><tr><td>{text}</td><td>短</td></tr></tbody></table>|<table><tbody><tr><td>{text}</td><td class="vtop">短</td></tr></tbody></table>'),
    ("106", "td vertical-align:middle（对照 top）", "markup", "", '<table><tbody><tr><td>{text}</td><td class="vtop">短</td></tr></tbody></table>|<table><tbody><tr><td>{text}</td><td class="vmid">短</td></tr></tbody></table>'),
    ("107", "ul list-style-type:decimal（对照没写，圆点）", "markup", "", '<ul><li>{text}</li></ul>|<ul class="lsd"><li>{text}</li></ul>'),
    ("108", "li list-style-type:cjk-ideographic", "markup", "", '<ul><li>{text}</li></ul>|<ul><li class="lsc">{text}</li></ul>'),
    ("109", "li list-style-type:none", "markup", "", '<ul><li>{text}</li></ul>|<ul><li class="lsn">{text}</li></ul>'),
    ("110", "ol（对照 ul）", "markup", "", '<ul><li>{text}</li></ul>|<ol><li>{text}</li></ol>'),
    ("111", "div height:6em（一行字，看后面那段）", "markup", "", '<div>短字</div>|<div class="h6e">短字</div>'),
    ("112", "p min-height:4em（一行字）", "markup", "", '<p>短字</p>|<p class="mh4">短字</p>'),
    ("113", "hr 缺省（对照没有）", "markup", "", '<p>短字</p>|<p>短字</p><hr/>'),
    ("114", "hr 1px 点线（《春雪》height:1px;border-style:none;border-top:1px dotted，对照缺省 hr）", "markup", "", '<p>短字</p><hr/>|<p>短字</p><hr class="hrd"/>'),
    ("115", "div display:flex;justify-content:center（里面一个 span）", "markup", "", '<div><span>短字</span></div>|<div class="flc"><span>短字</span></div>'),
    ("116", "p position:absolute;bottom:12%（《人生复本》出版社那行）", "markup", "", '<p>短字</p>|<p class="abs">短字</p>'),
    ("117", "td border:2px solid（类规则）、border-collapse:collapse（对照不合并）", "markup", "", '<table><tbody><tr><td class="tb">甲</td><td class="tb">乙</td></tr></tbody></table>|<table class="bc"><tbody><tr><td class="tb">甲</td><td class="tb">乙</td></tr></tbody></table>'),
    ("118", "div border-left:6px solid;padding-left:1em", "sheet", "border-left:6px solid #000;padding-left:1em", '<div{cls}>{text}</div>'),
    ("119", "div border:6px solid（全框）", "sheet", "border:6px solid #000", '<div{cls}>{text}</div>'),
    ("120", "<sup> 标签（短行）", "markup", "", '<p>甲乙丙丁</p>|<p>甲乙<sup>丙丁</sup></p>'),
    ("121", "<sub> 标签（短行）", "markup", "", '<p>甲乙丙丁</p>|<p>甲乙<sub>丙丁</sub></p>'),
    ("122", "span vertical-align:0.5em（短行）", "markup", "", '<p>甲乙<span>丙丁</span></p>|<p>甲乙<span class="vlen">丙丁</span></p>'),
    ("123", "span position:relative;top:-0.5em（短行）", "markup", "", '<p>甲乙<span>丙丁</span></p>|<p>甲乙<span class="vrel">丙丁</span></p>'),
    ("124", "p width:50%;margin:0 auto（对照只写宽度）", "markup", "", '<p class="w50">{text}</p>|<p class="w50 mauto">{text}</p>'),
    ("125", "空 span inline-block 19px 黑底圆（《狼厅》注释图标，对照没有）", "markup", "", '<p>甲乙丙丁戊</p>|<p>甲乙丙<span class="fnote"></span>丁戊</p>'),
    ("126", "同上、另加 display:inline、宽高 auto、背景和外边距清掉", "markup", "", '<p>甲乙丙丁戊</p>|<p>甲乙丙<span class="fnote fnoff"></span>丁戊</p>'),
    ("127", "td width:1em（《雪国》译者竖排格，对照没写）", "markup", "", '<table><tbody><tr><td>林少华译</td><td>右格</td></tr></tbody></table>|<table><tbody><tr><td class="tw1">林少华译</td><td>右格</td></tr></tbody></table>'),
    ("128", "td 行内 style width:1em", "markup", "", '<table><tbody><tr><td>林少华译</td><td>右格</td></tr></tbody></table>|<table><tbody><tr><td style="width:1em">林少华译</td><td>右格</td></tr></tbody></table>'),
    ("129", "td width=\"30\" 属性", "markup", "", '<table><tbody><tr><td>林少华译</td><td>右格</td></tr></tbody></table>|<table><tbody><tr><td width="30">林少华译</td><td>右格</td></tr></tbody></table>'),
    ("130", "图（rule.png 1000×6）行内 style width:12em（《克莱因壶》，Kindle 占 42%）", "markup", "", '<div><img class="ruleb" src="rule.png" alt=""/></div>|<div><img style="width:12em" src="rule.png" alt=""/></div>'),
    ("131", "图行内 style width:769px;max-width:428px（《狼厅》，Kindle 满宽）", "markup", "", '<div><img class="ruleb" src="rule.png" alt=""/></div>|<div><img style="max-width:428px;width:769px" src="rule.png" alt=""/></div>'),
    # 2026-10-14 三本武侠全集体检报的（《梁羽生》诗句每半句包 span.nw 1549 处、卷名 h1.bigtitle 34 处）
    ("132", "span white-space:nowrap（7 字一块，行尾放不下时整块挪到下一行？）", "markup", "", "<p>" + "<span>甲乙丙丁戊己，</span>" * 9 + "</p>|<p>" + '<span class="nw">甲乙丙丁戊己，</span>' * 9 + "</p>"),
    ("133", "h1 float:right（标题后接一段）", "sheet", "float:right", '<h1{cls}>女帝奇英传</h1>'),
    ("134", "h1 照《梁羽生》卷名 bigtitle（float:right、width:1.2em、上内边距 6em 配负上外边距、底色边框；对照是缺省 h1）", "markup", "", '<h1>女帝奇英传</h1>|<h1 class="bt">女帝奇英传</h1>'),
    # 2026-10-15 标题前后的距离（《人生海海》外层 line-height 是长度、标题字号大，Kindle 写出器继承成 0.6 行高；Move 照 0.6 排得很紧）
    ("135", "外层 line-height:1.2em 里 2em 的 h2（继承下来 0.6；对照外层没写）", "markup", "", '<div><h2>第一章</h2><p>{text}</p></div>|<div class="lh12"><h2>第一章</h2><p>{text}</p></div>'),
    ("136", "外层 line-height:1.2em 里 2em 的 p（对照外层没写）", "markup", "", '<div><p class="f2">第一章</p><p>{text}</p></div>|<div class="lh12"><p class="f2">第一章</p><p>{text}</p></div>'),
    ("137", "p line-height:0.6（两行字）", "sheet", "line-height:0.6", P),
    # 2026-10-15 Kindle 行高的下限（Q137 正文 0.6 只压到 0.74、Q135/Q136 大字几乎不压）：各字号 × 行高
    ("138", "p 1em，行高依次 不写/0.6/0.8/1.0/1.2（每段两行，量段内行距；A、B 一样，只看 Kindle）", "markup", "", '<p class="x1-0">测试文字测试文字测试文字测试文字测试文字测试文字测试文字测试文字</p><p class="x1-0_6">测试文字测试文字测试文字测试文字测试文字测试文字测试文字测试文字</p><p class="x1-0_8">测试文字测试文字测试文字测试文字测试文字测试文字测试文字测试文字</p><p class="x1-1_0">测试文字测试文字测试文字测试文字测试文字测试文字测试文字测试文字</p><p class="x1-1_2">测试文字测试文字测试文字测试文字测试文字测试文字测试文字测试文字</p>'),
    ("139", "p 1.5em，行高依次 不写/0.6/0.8/1.0/1.2（每段两行，量段内行距；A、B 一样，只看 Kindle）", "markup", "", '<p class="x1_5-0">测试文字测试文字测试文字测试文字测试文字测试</p><p class="x1_5-0_6">测试文字测试文字测试文字测试文字测试文字测试</p><p class="x1_5-0_8">测试文字测试文字测试文字测试文字测试文字测试</p><p class="x1_5-1_0">测试文字测试文字测试文字测试文字测试文字测试</p><p class="x1_5-1_2">测试文字测试文字测试文字测试文字测试文字测试</p>'),
    ("140", "p 2em，行高依次 不写/0.6/0.8/1.0/1.2（每段两行，量段内行距；A、B 一样，只看 Kindle）", "markup", "", '<p class="x2-0">测试文字测试文字测试文字测试文字</p><p class="x2-0_6">测试文字测试文字测试文字测试文字</p><p class="x2-0_8">测试文字测试文字测试文字测试文字</p><p class="x2-1_0">测试文字测试文字测试文字测试文字</p><p class="x2-1_2">测试文字测试文字测试文字测试文字</p>'),
    ("141", "p 3em，行高依次 不写/0.6/0.9（每段两行，量段内行距；A、B 一样，只看 Kindle）", "markup", "", '<p class="x3-0">测试文字测试文字测试文</p><p class="x3-0_6">测试文字测试文字测试文</p><p class="x3-0_9">测试文字测试文字测试文</p>'),
    ("142", "h2 2em，行高依次 不写/0.6/1.0（每段两行，量段内行距；A、B 一样，只看 Kindle）", "markup", "", '<h2 class="x2-0">测试文字测试文字测试文字测试文字</h2><h2 class="x2-0_6">测试文字测试文字测试文字测试文字</h2><h2 class="x2-1_0">测试文字测试文字测试文字测试文字</h2>'),
    # 2026-10-15 Move 上标题里的下边框横线图压到字上（《人生海海》版权页，标题行高照 Kindle 写 0.744）
    ("143", "h1 末尾放横线图，行内 line-height:0.744（对照没写）", "markup", "", '<h1>版权信息<img class="ruleb" src="rule.png" alt=""/></h1><p>{text}</p>|<h1 style="line-height:0.744">版权信息<img class="ruleb" src="rule.png" alt=""/></h1><p>{text}</p>'),
    ("144", "h1 末尾放横线图，行内 line-height:1（对照没写）", "markup", "", '<h1>版权信息<img class="ruleb" src="rule.png" alt=""/></h1><p>{text}</p>|<h1 style="line-height:1">版权信息<img class="ruleb" src="rule.png" alt=""/></h1><p>{text}</p>'),
    ("145", "div 2em 粗体末尾放横线图，行内 line-height:0.744（对照没写）", "markup", "", '<div class="d2">版权信息<img class="ruleb" src="rule.png" alt=""/></div><p>{text}</p>|<div class="d2" style="line-height:0.744">版权信息<img class="ruleb" src="rule.png" alt=""/></div><p>{text}</p>'),
    # 2026-10-16 体检里掌阅「没验证过」的《梁羽生》写法（版本历史页）：<style> 里行内元素逐字加框、p 浮动、块内不分页
    ("146", "<style> 里 span border:3px dashed、padding 6.4px 5px（《梁羽生》版本历史逐字加框）", "style", "border:3px dashed #000;padding:6.4px 5px", '<p>甲乙<span{cls}>丙</span>丁戊</p>'),
    ("147", "<style> 里 span border:3px dotted、padding 6.4px 5px（同上，点线）", "style", "border:3px dotted #000;padding:6.4px 5px", '<p>甲乙<span{cls}>丙</span>丁戊</p>'),
    ("148", "<style> 里 p float:right（《梁羽生》版本号 bver，后接一段）", "style", "float:right", '<p{cls}>壹</p><p>{text}</p>'),
    ("149", "<style> 里 div page-break-inside:avoid（前面垫 7 段、块 4 段，跨页）", "style", "page-break-inside:avoid", '<p>{text}</p>' * 7 + '<div{cls}>' + '<p>{text}</p>' * 4 + '</div>'),
    ("150", "外链 page-break-inside:avoid;break-inside:avoid（同上）", "sheet", "page-break-inside:avoid;break-inside:avoid", '<p>{text}</p>' * 7 + '<div{cls}>' + '<p>{text}</p>' * 4 + '</div>'),
    ("151", "居中的 h2（1.66667em）text-indent:48px（《古龙》calibre13，对照没写）", "markup", "", '<h2 class="hc">第一章</h2>|<h2 class="hc hi48">第一章</h2>'),
]

ALL_PROBES = PROBES

# 测试书全书共用的几条（"markup" 探针用）
BASE_CSS = [".mb0{margin-bottom:0;}", ".mt0{margin-top:0;}", ".mt001{margin-top:0.01em;}", ".ml2{margin-left:2em;}", ".mt2{margin-top:2em;}", ".mb2{margin-bottom:2em;}", ".mt1{margin-top:1em;}", "img.rule{width:100%;}", "img.ruleb{display:block;width:100%;}", ".bgd{background-color:#444444;color:#ffffff;margin-left:0;margin-right:0;}", ".nm{margin-left:-2em;margin-right:-2em;}", ".nl30{margin-left:-30px;}", ".nr{margin-right:-2em;}",
            ".sml2{margin-left:2em;}", ".sml30{margin-left:30px;}", ".spl2{padding-left:2em;}", ".spl30{padding-left:30px;}", "img.ri{width:3em;height:0.5em;}", ".im2{margin-left:2em;}", ".im30{margin-left:30px;}",
            ".lpt2{padding-top:2em;}", ".lmt2{margin-top:2em;}", ".tpl2{padding-left:2em;}",
            ".vsup{vertical-align:super;}", ".vsub{vertical-align:sub;}", "img.rt{width:10%;}", ".vmid{vertical-align:middle;}", ".vtop{vertical-align:top;}", ".vneg{vertical-align:-0.5em;}",
            ".lsd{list-style-type:decimal;}", ".lsc{list-style-type:cjk-ideographic;}", ".lsn{list-style-type:none;}", ".h6e{height:6em;}", ".mh4{min-height:4em;}",
            "hr.hrd{width:100%;height:1px;border-style:none;border-top:1px dotted gray;}", ".flc{display:flex;justify-content:center;align-items:center;}",
            ".abs{position:absolute;bottom:12%;left:0;width:100%;text-align:center;}", "td.tb{border:2px solid #000;}", "table.bc{border-collapse:collapse;}", ".vlen{vertical-align:0.5em;}", ".vrel{position:relative;top:-0.5em;}", ".w50{width:50%;}", ".mauto{margin-left:auto;margin-right:auto;}",
            "span.fnote{position:relative;display:inline-block;width:19px;height:19px;background-color:black;vertical-align:sub;border-radius:50%;margin:0 3px;}",
            "html body span.fnoff{display:inline;width:auto;height:auto;background-color:transparent;background-image:none;margin:0;}", "td.tw1{width:1em;}", ".nw{white-space:nowrap;}", ".lh12{line-height:1.2em;}", "div.d2{font-size:2em;font-weight:bold;}", "p.f2{font-size:2em;}", "p.x1-0{font-size:1em;}", "p.x1-0_6{font-size:1em;line-height:0.6;}", "p.x1-0_8{font-size:1em;line-height:0.8;}", "p.x1-1_0{font-size:1em;line-height:1.0;}", "p.x1-1_2{font-size:1em;line-height:1.2;}", "p.x1_5-0{font-size:1.5em;}", "p.x1_5-0_6{font-size:1.5em;line-height:0.6;}", "p.x1_5-0_8{font-size:1.5em;line-height:0.8;}", "p.x1_5-1_0{font-size:1.5em;line-height:1.0;}", "p.x1_5-1_2{font-size:1.5em;line-height:1.2;}", "p.x2-0{font-size:2em;}", "p.x2-0_6{font-size:2em;line-height:0.6;}", "p.x2-0_8{font-size:2em;line-height:0.8;}", "p.x2-1_0{font-size:2em;line-height:1.0;}", "p.x2-1_2{font-size:2em;line-height:1.2;}", "p.x3-0{font-size:3em;}", "p.x3-0_6{font-size:3em;line-height:0.6;}", "p.x3-0_9{font-size:3em;line-height:0.9;}", "h2.x2-0{font-size:2em;}", "h2.x2-0_6{font-size:2em;line-height:0.6;}", "h2.x2-1_0{font-size:2em;line-height:1.0;}",
            "h2.hc{display:block;font-size:1.66667em;font-weight:bold;line-height:1.2;text-align:center;margin:0.83em 0;}", ".hi48{text-indent:48px;}",
            "h1.bt{font-size:300%;font-weight:normal;color:#1B272E;text-align:center;width:1.2em;line-height:120%;background:#E5E2D3;border:solid #DFDBCA 5px;float:right;padding:6em 12px 12px 12px;margin:-6em 10px auto auto;}"]


def page(code, cid, body, head_style="", barcode=None):
    style = f"<style>{head_style}</style>" if head_style else ""
    mark = f'<img class="code" src="{barcode}" alt=""/>' if barcode else code
    return f'''<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="zh" xml:lang="zh">
<head><title>{code}</title><link rel="stylesheet" type="text/css" href="s.css"/>{style}</head>
<body><p id="{cid}">{mark}</p><p>{BEFORE}</p>{body}<p>{AFTER}</p></body></html>'''


CODE_CELLS = 11  # 条码格数：黑、8 位序号、A/B、黑（2026-11-01 以前 7 位，探针过了 127 个，128 以后和前面的重号）
CODE_BITS = CODE_CELLS - 3


def code_png(n, b):
    """第 n 个探针（从 1 起）的条码：CODE_CELLS 格、每格 10×10 像素，黑 = 1。"""
    import struct
    import zlib
    assert 0 < n < 1 << CODE_BITS, n
    bits = [1] + [(n >> (CODE_BITS - 1 - i)) & 1 for i in range(CODE_BITS)] + [int(b), 1]
    row = b"\x00" + b"".join((b"\x00" if bit else b"\xff") * 10 for bit in bits)
    raw = row * 10
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 10 * len(bits), 10, 8, 0, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


# 截屏里只看上沿在这以上的行，下面是页脚（页码、时间；`--max-y` 可改）。Ocean 5 Pro 1680 高时是 1600；Air 3 Pro 1920 高，
# 按同样的比例推断 1828（2026-11-02 真机：页脚一行上沿 1874，1828 够用）
MAX_Y = 1828
# 上沿不超过这个数（在屏幕上方）的是页眉，不看（`--min-y` 可改；Air 3 Pro 自带阅读器顶上一行书名、章节名和左上角「>」，下沿 79；正文第一行上沿 98，2026-11-02 截屏量的）
MIN_Y = 90


def decode_code(png):
    """截屏里第一行（条码）认出 (探针序号, 是不是 B)；认不出 None。"""
    from PIL import Image
    ls = [l for l in lines_of(png) if MIN_Y < l[0] < MAX_Y]
    if not ls:
        return None
    top, bottom, left, right = ls[0]
    im = Image.open(png).convert("L")
    y = (top + bottom) // 2
    w = (right - left + 1) / CODE_CELLS
    bits = [im.getpixel((int(left + (i + 0.5) * w), y)) < 128 for i in range(CODE_CELLS)]
    # 条码只在章首页顶上（上沿 98）；一章排不下一页时（Air 3 Pro 缺省字号大）续页第一行是大字正文，可能碰巧认成条码（2026-11-02 Q140 续页
    # 上沿 132、认成 64）。续页首行正好也在 98 附近时拦不住，靠两端黑格、行高再筛
    if not (bits[0] and bits[-1]) or bottom - top < 10 or (MIN_Y and top > MIN_Y + 40):
        return None
    n = sum(1 << (CODE_BITS - 1 - i) for i in range(CODE_BITS) if bits[1 + i])
    return n, bits[1 + CODE_BITS]


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
<manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="css" href="s.css" media-type="text/css"/><item id="rule" href="rule.png" media-type="image/png"/><item id="tall" href="tall.png" media-type="image/png"/>{codes}{manifest}</manifest>
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
        z.writestr("OEBPS/tall.png", rule_png(20, 40), compress_type=zipfile.ZIP_STORED)  # 竖长的黑块：看行内图的竖直对齐（Move 只认图的百分比宽度）
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
    # 条码只有 7 位序号（1–127），再多就绕回 0、1 去，认错页（2026-11-02 全跑 151 个时 Q128 认成 0、Q129 认成 1）：分几次用 --only 跑
    if len(PROBES) > 127:
        sys.exit(f"一次最多 127 个探针（现在 {len(PROBES)} 个）：分开跑，如 --only=1-127、--only=128-{int(PROBES[-1][0])}")
    work = tempfile.mkdtemp(prefix="quirks-ir-")
    # 每次换个文件名：同名的书掌阅记着上次读到哪，会从那里接着翻
    name = f"quirks-ir-{time.strftime('%m%d%H%M%S')}"
    book = os.path.join(work, f"{name}.epub")
    make(book, "测试-怪癖-掌阅", barcode=True, device="ireader")
    adb("shell", "mkdir", "-p", "/sdcard/documents/测试")
    adb("push", book, f"/sdcard/documents/测试/{name}.epub")
    adb("shell", "am", "start", "-n", "com.android.documentsui/com.szzy.ireader.ink.fileexplorer.ui.activity.MainActivity")
    time.sleep(3)
    # 文件管理器停在「测试」里时列表不刷新：点面包屑回 documents 再进来；在哪一层都试几次。停在别的目录（上次看书的「经典名著」）时
    # 面包屑在 uiautomator 里没有文字，按返回键往上走一层（2026-10-16）
    for _ in range(8):
        if tap_text(name):
            break
        if not tap_text("测试") and not tap_text("documents") and not tap_text("存储空间"):
            adb("shell", "input", "keyevent", "KEYCODE_BACK")
        time.sleep(2)
    else:
        sys.exit(f"文件管理器里没找到「{name}」")
    # Air 3 Pro 打开 300 多页的书要排版好一阵，这期间的滑动不翻页、截屏也不变（2026-11-02）
    time.sleep(20)
    codes = {i: c for i, (c, *_) in enumerate(PROBES, 1)}
    out, seen, misses, dups = {}, set(), 0, 0
    for k in range(2 * len(PROBES) + 60):  # 多留些：Air 3 Pro 字大，有的章排两页
        png = os.path.join(work, f"p{k}.png")
        adb("exec-out", "screencap", "-p", out=png)
        got = decode_code(png)
        if got is None:
            dups = 0
            misses += 1
            if misses > 5:
                break
        elif got in seen:
            # 翻页没翻过去：头一回只是再滑一次（2026-11-02 Air 3 Pro 在 Q19B 偶尔一滑没翻，这时按返回键就退出了阅读器）；
            # 连着第二回才当是滑到字上弹出了菜单，按返回键退出菜单。连着几次就是到头了
            # 单独数连着重复几回：续页（没条码）记的 misses 不算，免得续页后头一回重复就按返回键
            # 只在连着第二回重复时按一次返回键关菜单；再按就退出阅读器了
            if dups == 1:
                adb("shell", "input", "keyevent", "KEYCODE_BACK")
            dups += 1
            misses += 1
            if misses > 5:
                break
        else:
            misses = dups = 0
            seen.add(got)
            n, b = got
            if n in codes:
                out.setdefault(codes[n], {})["B" if b else "A"] = [l for l in lines_of(png) if MIN_Y < l[0] < MAX_Y]
            if len(seen) == 2 * len(PROBES):
                break
        # 右往左滑翻下一页：Ocean 5 Pro 1264×1680 时是 1100→150、y 1580；Air 3 Pro 页脚上沿 1874，滑 y 1700（页底空白，2026-11-02 真机试过能翻）
        adb("shell", "input", "swipe", "1250", "1700", "170", "1700", "200")
        time.sleep(2)
    print(f"掌阅：认出 {len(seen)} 页（共 {2 * len(PROBES)}）；截屏在 {work}；测试书 documents/测试/{name}.epub 用完请删", file=sys.stderr)
    return out


# ---------------------------------------------------------------- Kobo（2026-11-01，未上真机）
# Kobo 没有 adb：生成、拷进去、手动截屏、拷回来、量，分开几步。截屏的开法和文件名来自社区资料，没在真机上验证：
# `.kobo/Kobo/Kobo eReader.conf` 的 [FeatureSettings] 加 `Screenshots=true`（维护者自己改，本脚本不碰 .kobo/），按电源键截屏，
# 存到存储根目录、名字像 screen_001.png（--pattern= 改）。
KOBO_W, KOBO_H = 1072, 1448
REPO = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..")
KOBO_TOML = os.path.join(REPO, "crates", "profile", "profiles", "kobo.toml")
FORMATS = ("epub", "kepub")


def kobo_deliver(path=KOBO_TOML):
    """kobo.toml 的 [deliver]：(卷标, 产物目录)。"""
    import tomllib
    with open(path, "rb") as f:
        d = tomllib.load(f)["deliver"]
    if d.get("kind") != "usbms":
        sys.exit(f"{path} 的 [deliver] 不是 usbms")
    return d["label"], d["dir"]


def usbms_base(env=None, exists=os.path.isdir):
    """同 booklib（library::deliver::DeviceEnv）：$BOOKLIB_USBMS_DIR；没设就是 /run/media/$USER，它不在、/media/$USER 在时用后者。"""
    env = os.environ if env is None else env
    if env.get("BOOKLIB_USBMS_DIR"):
        return env["BOOKLIB_USBMS_DIR"]
    user = env.get("USER", "")
    run, media = os.path.join("/run/media", user), os.path.join("/media", user)
    return media if not exists(run) and user and exists(media) else run


def mounted(point):
    """同 booklib：挂载点和上一级不在同一个文件系统才算挂着（拔掉后剩下的空目录不算）。"""
    try:
        return os.stat(point).st_dev != os.stat(os.path.dirname(os.path.abspath(point))).st_dev
    except OSError:
        return False


def kobo_mount(env=None, mount_check=None):
    """Kobo 挂载点；没接上退出。只认 <base>/<卷标>（不跟 <卷标>1 这种，同 booklib）。
    挂载检查同 booklib：$BOOKLIB_MTP_NO_MOUNT_CHECK=1 时关（单测用普通目录当设备）。"""
    env = os.environ if env is None else env
    if mount_check is None:
        mount_check = env.get("BOOKLIB_MTP_NO_MOUNT_CHECK", "0") in ("", "0")
    label, _ = kobo_deliver()
    point = os.path.join(usbms_base(env), label)
    if mount_check and not mounted(point):
        sys.exit(f"Kobo 没接上（{point} 没挂上）")
    if not os.path.isdir(point):
        sys.exit(f"Kobo 没接上（{point} 不在）")
    return point


def kobo_names(stamp):
    """一次运行的文件名：{格式: 文件名}。每次换名字（同名的书阅读器可能记着读到哪）；KEPUB 要 .kepub.epub 结尾 Nickel 才用 Kobo WebKit 开。"""
    name = f"quirks-kb-{stamp}"
    return {"epub": f"{name}.epub", "kepub": f"{name}.kepub.epub"}


def safe_dest(mount, rel):
    """设备上要写的路径；不许落到 .kobo/ 或挂载点外面。"""
    dest = os.path.normpath(os.path.join(mount, rel))
    parts = os.path.relpath(dest, mount).split(os.sep)
    if parts[0] in ("..", ".") or any(x.startswith(".") for x in parts):
        sys.exit(f"不往 {dest} 写（在挂载点外或隐藏目录里）")
    return dest


def run_dir(opts, latest=True):
    """--run= 指的目录；没给时用 target/quirks-kobo/ 下最新的一次。"""
    if opts.get("--run"):
        return opts["--run"]
    base = os.path.join(REPO, "target", "quirks-kobo")
    runs = sorted(d for d in os.listdir(base) if os.path.isfile(os.path.join(base, d, "run.json"))) if os.path.isdir(base) else []
    if not runs:
        sys.exit("还没有 kobo generate 过（或用 --run= 指目录）")
    return os.path.join(base, runs[-1])


def load_run(d):
    with open(os.path.join(d, "run.json"), encoding="utf-8") as f:
        return json.load(f)


def save_run(d, r):
    with open(os.path.join(d, "run.json"), "w", encoding="utf-8") as f:
        json.dump(r, f, ensure_ascii=False, indent=1)


def kobo_generate(opts):
    stamp = time.strftime("%m%d%H%M%S")
    names = kobo_names(stamp)
    d = opts.get("--run") or os.path.join(REPO, "target", "quirks-kobo", stamp)
    if os.path.exists(os.path.join(d, "run.json")):
        sys.exit(f"{d} 已经生成过（会冲掉拷回截屏的记录），换个 --run= 或等一秒")
    os.makedirs(d, exist_ok=True)
    bindir = opts.get("--bin") or os.path.join(REPO, "target", "release")
    epub = os.path.join(d, names["epub"])
    make(epub, "测试-怪癖-Kobo-EPUB", barcode=True, device="kobo")
    # KEPUB 另起一本（书名不同，书架上分得开），和书库一样：先按 kobo 模式优化（给了 --optimize 时），再转
    src = os.path.join(d, "kepub-src.epub")
    make(src, "测试-怪癖-Kobo-KEPUB", barcode=True, device="kobo")
    subprocess.run([os.path.join(bindir, "epub-to-kepub"), src, os.path.join(d, names["kepub"])], check=True)
    for f in (src, src + ".raw.epub", epub + ".raw.epub"):  # make 先写的没优化版（给了 --optimize 时）
        if os.path.exists(f):
            os.remove(f)
    save_run(d, {"stamp": stamp, "files": names, "optimized": bool(OPTIMIZE), "only": opts.get("--only"), "collected": {}})
    print(f"生成好了：{d}\n  {names['epub']}\n  {names['kepub']}\n下一步：插上 Kobo，probe.py kobo push --run={d}")


def kobo_push(opts):
    d = run_dir(opts)
    r = load_run(d)
    mount = kobo_mount()
    _, sub = kobo_deliver()
    dest = safe_dest(mount, os.path.join(sub, "测试"))
    import shutil
    os.makedirs(dest, exist_ok=True)
    for f in r["files"].values():
        tmp = os.path.join(dest, f + ".part")  # 先写临时名再改名：拷一半拔掉不留半本书（.part Nickel 不认）
        shutil.copyfile(os.path.join(d, f), tmp)
        os.replace(tmp, os.path.join(dest, f))
    os.sync()
    print(f"""已拷进 {dest}：{', '.join(r['files'].values())}
接下来手动（截屏要先在 .kobo/Kobo/Kobo eReader.conf 的 [FeatureSettings] 下加 Screenshots=true，自己改；未验证）：
  1. 弹出：udisksctl unmount -b <设备>（或文件管理器里弹出），拔线，等 Kobo 导入完
  2. 先开「测试-怪癖-Kobo-EPUB」：从第一页起每页按一下电源键截屏、翻页，到最后一章「filler」为止；字号、边距别动
  3. 插上，probe.py kobo collect --format=epub --run={d}
  4. 再开「测试-怪癖-Kobo-KEPUB」照做，然后 probe.py kobo collect --format=kepub --run={d}
测试书留在 Kobo 上，用完自己删（在 Kobo 上删或删 {dest} 里的文件）""")


def file_sha256(path):
    """文件内容的 SHA-256（十六进制），分块读。"""
    import hashlib
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def kobo_collect(opts, fmt):
    d = run_dir(opts)
    r = load_run(d)
    mount = kobo_mount()
    import fnmatch
    import shutil
    pat = opts.get("--pattern", "screen_*.png")
    # 只看存储根目录（截屏存在那，未验证），不往下找
    # fnmatch 的 * 也匹配开头的点：隐藏文件、符号链接一律不要（--clean 会删）
    shots = sorted(f for f in os.listdir(mount) if fnmatch.fnmatch(f, pat) and not f.startswith(".")
                   and not os.path.islink(os.path.join(mount, f)) and os.path.isfile(os.path.join(mount, f)))
    # 去重按内容哈希（记成 `名字:sha256`，只比哈希）：--clean 后 Kobo 可能又从 screen_001 编号，按 `名字:大小` 比会把
    # 同名同大小的新截屏当成拷过而漏掉（agy 发现）。2026-11-03 以前的 `名字:大小` 记录对不上任何哈希，不用迁移
    # 别的格式拷过的只认 `名字:哈希` 都对上的（没 --clean 时设备上留着的上一格式截屏）：两种格式渲染出逐像素一样的页时
    # 只比哈希会把这一格式的那页漏掉（agy 二审）
    seen = {k.split(":")[-1] for k in r["collected"].get(fmt, [])}
    other = {k for g, v in r["collected"].items() if g != fmt for k in v}
    sums = {f: file_sha256(os.path.join(mount, f)) for f in shots}
    new = [f for f in shots if sums[f] not in seen and f"{f}:{sums[f]}" not in other]
    out = os.path.join(d, "shots", fmt)
    os.makedirs(out, exist_ok=True)
    got, names = [], []
    for f in new:
        src = os.path.join(mount, f)
        dst = os.path.join(out, f)
        if os.path.exists(dst):
            # --clean 后重新编号的同名新截屏：不覆盖上一批（kobo_measure_dir 每次量整个目录），名字后加哈希前 8 位
            stem, ext = os.path.splitext(f)
            dst = os.path.join(out, f"{stem}-{sums[f][:8]}{ext}")
        shutil.copy2(src, dst)
        if file_sha256(dst) != sums[f]:
            sys.exit(f"{f} 拷回来内容不对")
        got.append(f"{f}:{sums[f]}")
        names.append(f)
    r["collected"].setdefault(fmt, []).extend(got)
    save_run(d, r)
    print(f"拷回 {len(got)} 张截屏（设备根目录共 {len(shots)} 张对得上 {pat}）→ {out}", file=sys.stderr)
    if "--clean" in sys.argv[1:]:
        for f in names:
            os.remove(os.path.join(mount, f))
        os.sync()
        print(f"删了设备上拷回来的 {len(got)} 张", file=sys.stderr)
    return kobo_measure_dir(out, os.path.join(d, f"result-{fmt}.json"))


def kobo_measure_dir(src, save=None):
    """一个目录里的截屏（按文件名排）逐张认条码、量行。返回同 [`ireader_run`]。"""
    from PIL import Image
    codes = {i: c for i, (c, *_) in enumerate(PROBES, 1)}
    out, seen, unknown = {}, set(), 0
    for f in sorted(os.listdir(src)):
        if not f.lower().endswith(".png"):
            continue
        png = os.path.join(src, f)
        try:
            with Image.open(png) as im:
                size = im.size
        except (OSError, SyntaxError) as e:
            print(f"⚠ {f} 读不了（{e}），跳过", file=sys.stderr)
            unknown += 1
            continue
        if size != (KOBO_W, KOBO_H):
            print(f"⚠ {f} 是 {size[0]}×{size[1]}，不是 {KOBO_W}×{KOBO_H}（横屏？）", file=sys.stderr)
        got = decode_code(png)
        if got is None or got[0] not in codes:
            unknown += 1
            continue
        if got in seen:
            print(f"⚠ {f} 和前面一张是同一页 {got}，用前面那张", file=sys.stderr)
            continue
        seen.add(got)
        out.setdefault(codes[got[0]], {})["B" if got[1] else "A"] = [l for l in lines_of(png) if MIN_Y < l[0] < MAX_Y]
    print(f"认出 {len(seen)} 页（共 {2 * len(PROBES)}），认不出 {unknown} 张", file=sys.stderr)
    if save:
        with open(save, "w", encoding="utf-8") as fh:
            json.dump(out, fh, ensure_ascii=False)
    return out


def kobo_report(d):
    """两种格式并排。不判 same/fixed：要和 Kindle 比（bookconv::kindle_model），结论人看了以后手写进 css-quirks.toml 的 kobo 一栏。"""
    res = {}
    for fmt in FORMATS:
        p = os.path.join(d, f"result-{fmt}.json")
        res[fmt] = {}
        if os.path.exists(p):
            with open(p, encoding="utf-8") as f:
                res[fmt] = {c: r for c, _, r in report(json.load(f))}
    print("编号\t说明\tEPUB\tKEPUB")
    for code, desc, *_ in PROBES:
        print(f"Q{code}\t{desc}\t{res['epub'].get(code, '没量')}\t{res['kepub'].get(code, '没量')}")


def use_run_probes(d, opts):
    """条码是探针在这次测试书里的序号：量的时候探针要和 generate 时一样（run.json 记着 --only）。"""
    global PROBES
    only = load_run(d).get("only")
    if "--only" in opts and opts["--only"] != only:
        sys.exit(f"这次测试书是 --only={only} 生成的，量的时候不能换（条码是序号）")
    PROBES = select(only) if only else ALL_PROBES


def kobo_main(args, opts):
    global MAX_Y, MIN_Y
    MAX_Y = int(opts.get("--max-y", KOBO_H * 95 // 100))  # 推断：页脚（页码、进度）在最下面 5% 里，未上真机
    # 页眉高度没量过：缺省不去页眉（MIN_Y 是掌阅 Air 3 Pro 的值，用在 Kobo 上会把条码当页眉丢掉）
    MIN_Y = int(opts.get("--min-y", 0))
    sub = args[1] if len(args) > 1 else ""
    fmt = opts.get("--format")
    if sub in ("collect", "measure") and fmt not in FORMATS:
        sys.exit("要写 --format=epub 或 --format=kepub（两本书的截屏分开拷、分开量）")
    if sub == "generate":
        kobo_generate(opts)
        return
    if sub == "push":
        kobo_push(opts)
        return
    if sub in ("collect", "report") or (sub == "measure" and not opts.get("--from-dir")):
        use_run_probes(run_dir(opts), opts)
    if sub == "report":
        kobo_report(run_dir(opts))
        return
    if sub == "collect":
        m = kobo_collect(opts, fmt)
    elif sub == "measure":
        if opts.get("--from-dir"):
            if "--only" not in opts:
                print("⚠ --from-dir 按全部探针认条码；测试书是 --only= 生成的话要带同样的 --only=", file=sys.stderr)
            m = kobo_measure_dir(opts["--from-dir"])
        else:
            d = run_dir(opts)
            m = kobo_measure_dir(os.path.join(d, "shots", fmt), os.path.join(d, f"result-{fmt}.json"))
    else:
        sys.exit(__doc__)
    if "--json" in sys.argv[1:]:
        print(json.dumps(m, ensure_ascii=False))
        return
    for code, desc, r in report(m):
        print(f"Q{code}\t{desc}\t{r}")


def select(spec, probes=None):
    """--only=88-99 或 13,28：只留这几个探针。"""
    want = set()
    for part in spec.split(","):
        lo, _, hi = part.partition("-")
        want.update(range(int(lo), int(hi or lo) + 1))
    return [p for p in (probes or ALL_PROBES) if int(p[0]) in want]


def main():
    global PROBES, OPTIMIZE, MAX_Y, MIN_Y
    args, opts = cli_args(1, __doc__), cli_opts()
    cmd = args[0]
    # --only=88-99 或 --only=13,28：只跑这几个探针（掌阅全跑一遍要 10 分钟）
    if "--only" in opts:
        PROBES = select(opts["--only"])
    OPTIMIZE = opts.get("--optimize")
    MAX_Y = int(opts.get("--max-y", MAX_Y))
    MIN_Y = int(opts.get("--min-y", MIN_Y))
    if cmd == "make" and len(args) == 2:
        make(args[1])
    elif cmd == "kobo":
        kobo_main(args, opts)
    elif cmd == "ireader":
        m = ireader_run()
        if "--json" in sys.argv[1:]:
            print(json.dumps(m, ensure_ascii=False))
            return
        for code, desc, r in report(m):
            print(f"Q{code}\t{desc}\t{r}")
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
