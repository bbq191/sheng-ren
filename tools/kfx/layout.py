"""三台版式检查：以 Kindle 写出器的 KFX 为准，查掌阅、Move 的 EPUB 有没有排成一样的版式（不比字体、字号、间距的数值）。
用法：python3 layout.py <书目录> <工作目录> [--bin=target/release] [--reuse] [--top=8] [--baseline=tools/kfx/layout-baseline.tsv] [--update] [书名子串…]

2026-10-15 用户定的重点：「该居中的居中、该竖排的竖排、该有横杠的有横杠、合集分书封面不多页……这就基本能保持三端观感一致」——
字体能随手改，字号、间距的数值差一点看不出来（那些归 unify.py）；这里一处不一样就列出来，不打总分。查的是：

- 对齐：同一个文字块在三台是居中、靠右还是靠左（左对齐和两端对齐算一样）；
- 竖排：一行只放得下一两个字的窄块（《梁羽生》卷名）另外两台也得窄，反过来也一样；块宽差出页宽 10% 以上的另列一项；
- 横线：块上下的边框、左右的竖线、`<hr>`，Kindle 画了的两台也要画（Move 不画 CSS 边框：上下边框数 `wash/box_fix.rs` 插的
  横线图，左右的数单元格边框；掌阅照 CSS）；
- 封面页：只有图的节（封面、合集里每本的分书封面、整页插图）的个数、位置（前面有几个有字的节）和 Kindle 一样，不多翻一页
  （`break_margin` 照 Kindle 估算插的分页并回前一节，见 `estimated_breaks`）；
- 分页：哪些块另起一页（文件开头、强制分页）三台一样；Move 文件开头的块上有强制分页会多一页空白（KINDLE_RULES 34 修了，这里
  兜底）；两台有没有内容的节（排成一页空白）；Move 不认 `break-before:page`（只认 `page-break-before`）。

Kindle 一边读 KFX 的版面树（`kfx-dump`），掌阅、Move 读 `epub-to-kfx --styles --device=…` 的块树（`#sec`、`#open`、`#box`、
`#rule`、`#hr` 行）。阅读器的怪癖按 profile 折进来：`padding = false` 不算内边距、`block_width = false` 不算块宽、`borders = false`
只算表格里的边框。表格里的块不比块宽（列宽另有规矩，见 docs/devices.md）。文字块按前 60 个字配对（同 unify.py）。
门槛：基线文件记着每本书每台各项的处数，比记下的多就算变差（退出码 1）；没记着的书有一处就不过。--update 写回。"""
import collections, math, os, re, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kfx import by_type, cli_args, cli_opts, pools, reading_order
from s2kcmp import NAMES, fmt
from s2kdev import kfx_features
import toollib
from unify import DEVICES, READER_GEOMETRY, books_in, profile_flag, profile_value

PAGE = 32.0  # Kindle 一行的根 em 数（同 s2kdev.py）
CATS = ['对齐', '竖排', '块宽', '横线', '分页', '空白页', '封面', '怪癖']
KEY = lambda t: ''.join(t.split())[:60]
# KFX 边框：槽 0 是四边，1–4 是上、左、下、右（`crates/kfx/src/write/style.rs` 的 `border_props`）
B_STYLE, B_WIDTH, B_COLOR = ['$88', '$89', '$90', '$91', '$92'], ['$93', '$94', '$95', '$96', '$97'], ['$83', '$84', '$85', '$86', '$87']
SLOT_SIDE = {1: 'T', 2: 'L', 3: 'B', 4: 'R'}
TABLE = {'$278', '$151', '$454', '$455', '$279'}


def lum(argb):
    """颜色（ARGB 整数）的灰度 0–255。"""
    r, g, b = (argb >> 16) & 255, (argb >> 8) & 255, argb & 255
    return 0.299 * r + 0.587 * g + 0.114 * b


def contrasting(sides, colors, bg):
    """看得出来的边：边框颜色和盒子底色（没有底色是白页）灰度差 24 以上（墨水屏 16 级灰差一级多）。
    《梁羽生》卷名框 `border: solid #DFDBCA 5px` 套在 `#E5E2D3` 底色上，屏幕上分不出来，只看得见底色的边。"""
    base = lum(bg) if bg is not None else 255.0
    return ''.join(c if c != '-' and abs(lum(col if col is not None else 0xFF000000) - base) >= 24 else '-' for c, col in zip(sides, colors))


def text_len(t):
    """字数（全角算 1、半角算 0.5）。"""
    return sum(0.5 if ord(c) < 0x2E80 else 1.0 for c in t)


# ---------- Kindle：KFX 版面树 → 事件

def kfx_events(path, base_fs):
    """[(事件, …)]：('sec',) 每节开头；('open', 在表格里, 盒子) / ('close',)；('text', 文字, 特征, 盒子, 横线图)；('img', 宽, 高)；('hr',)。
    盒子：{'ml','mr','pl','pr'（各是 (根 em 占页宽的比例, 包含块百分比)），'width'（同样的二元组或 None），'sides'（看得见的边框字母）}。"""
    E = by_type(path)
    styles = E.get('$157', {})
    pl = pools(E)
    res = {v['$175']: v for v in E.get('$164', {}).values() if isinstance(v, dict) and '$175' in v}
    out = []

    def raw(n):
        return styles.get(str(n.get('$157')), {}) if '$157' in n else {}

    def size_of(chain):
        return next((d['$16']['$307'] for d in reversed([raw(n) for n in chain]) if isinstance(d.get('$16'), dict)), 1.0)

    def length(v, size):
        if not isinstance(v, dict) or '$307' not in v:
            return (0.0, 0.0)
        x, u = float(v['$307']), v.get('$306')
        if u == '$314':
            return (0.0, x / 100)
        if u == '$505':
            return (x * base_fs / PAGE, 0.0)
        if u == '$318':
            return (x / 12 * base_fs / PAGE, 0.0)
        return (x * size * base_fs / PAGE, 0.0)

    def box(n, chain):
        st, size = raw(n), size_of(chain)
        sides, colors = set(), {}
        for slot in range(5):
            if B_STYLE[slot] not in st:
                continue
            w = st.get(B_WIDTH[slot])
            if isinstance(w, dict) and float(w.get('$307', 1)) <= 0 or st.get(B_COLOR[slot]) == '$349':
                continue
            col = st.get(B_COLOR[slot]) if isinstance(st.get(B_COLOR[slot]), int) else None
            for c in ('TRBL' if slot == 0 else SLOT_SIDE[slot]):
                sides.add(c)
                colors[c] = col
        bg = st.get('$70') if isinstance(st.get('$70'), int) else None
        sides = contrasting(''.join(c if c in sides else '-' for c in 'TRBL'), [colors.get(c) for c in 'TRBL'], bg)
        return {'ml': length(st.get('$48'), size), 'mr': length(st.get('$50'), size), 'pl': length(st.get('$53'), size),
                'pr': length(st.get('$55'), size), 'width': length(st['$56'], size) if '$56' in st else None,
                'sides': sides}

    def fmt_chain(chain):
        return [{NAMES.get(k, k): fmt(v) for k, v in raw(n).items() if k not in ('$173', '$10')} for n in chain]

    def walk(n, chain, in_table):
        chain = chain + [n]
        ty = n.get('$159')
        t = n.get('$145')
        if isinstance(t, dict) and t.get('name') in pl:
            out.append(('text', pl[t['name']][t['$403']], kfx_features(fmt_chain(chain), base_fs), box(n, chain), []))
            return
        if ty == '$269' and isinstance(n.get('$146'), list) and any(isinstance(k, str) for k in n['$146']):
            # 有行内图片的文字节点：字直接写在内容列表里，和图片节点交错（写出器 20）
            text = ''.join(k for k in n['$146'] if isinstance(k, str))
            out.append(('text', text, kfx_features(fmt_chain(chain), base_fs), box(n, chain), []))
            return
        if ty == '$271':
            meta = res.get(n.get('$175'), {})
            out.append(('img', meta.get('$422', 0), meta.get('$423', 0)))
            return
        if ty == '$596':
            out.append(('hr',))
            return
        kids = [k for k in n.get('$146', []) or [] if isinstance(k, dict)]
        tbl = in_table or ty in TABLE
        out.append(('open', tbl, box(n, chain)))
        for k in kids:
            walk(k, chain, tbl)
        out.append(('close',))

    for s in reading_order(E):
        out.append(('sec', False))
        for pt in E['$260'][s]['$141']:
            sl = E['$259'].get(pt.get('$176'))
            for n in (sl or {}).get('$146', []):
                walk(n, [], False)
    return out


# ---------- 掌阅、Move：epub-to-kfx --styles 的块树 → 事件

def epub_events(path, bindir, device):
    """同 [`kfx_events`]，外加 ('sec', 文件开头的块上有强制分页)；返回 (正文根 em 字号, 事件)。盒子的 em 是本书的根 em。"""
    lines = subprocess.run([f'{bindir}/epub-to-kfx', '--styles', f'--device={device}', path], check=True, capture_output=True, text=True).stdout.splitlines()
    bfs = float(lines[0].split('\t')[1])
    out, depth_table = [], [False]

    def hz(s):
        em, pct = (float(x) for x in s.split('+'))
        return (em, pct / 100)

    def parse_box(f):
        w = f[4]
        width = None if w == '-' else (float(w[3:]), 0.0) if w.startswith('em:') else (0.0, float(w[2:]) / 100) if w.startswith('%:') else (float(w[3:]) / 12, 0.0)
        hexc = lambda x: None if x == '-' else int(x, 16)
        sides = contrasting(f[5], [hexc(x) for x in f[6].split(',')], hexc(f[7]))
        return {'ml': hz(f[0]), 'mr': hz(f[1]), 'pl': hz(f[2]), 'pr': hz(f[3]), 'width': width, 'sides': sides}

    for line in lines[1:]:
        f = line.split('\t')
        if f[0] == '#sec':
            out.append(('sec', f[3] == 'true', f[2] == 'true'))
        elif f[0] == '#open':
            tbl = depth_table[-1] or int(f[1]) in (278, 151, 454, 455, 279)
            depth_table.append(tbl)
            out.append(('open', tbl, parse_box(f[2:])))
        elif f[0] == '#close':
            depth_table.pop()
            out.append(('close',))
        elif f[0] == '#hr':
            out.append(('hr',))
        elif f[0] == '#box' and out and out[-1][0] == 'text':
            out[-1][3].update(parse_box(f[1:]))
        elif f[0] == '#rule' and out and out[-1][0] == 'text':
            out[-1][4].append(f[1])
        elif f[0] == '#img' and len(f) == 8 and f[7] == 'block':
            name = f[1].rsplit('/', 1)[-1]
            # 我们插的横线图（Move 的边框）当横线；垫图、撑高的窄图不是内容（`top_spacer.rs`）
            if name.startswith('eink-rule-'):
                out.append(('rule',))
            elif not name.startswith('eink-'):
                out.append(('img', int(f[2]), int(f[3]), f[1]))
        elif not f[0].startswith('#') and len(f) == 13:
            fs = float(f[1])
            align = {'-': 'left', 'start': 'left', 'end': 'right'}.get(f[7], f[7])
            out.append(('text', f[0], {'fs': fs, 'align': align}, {}, []))
    return bfs, out


# ---------- 事件 → 每个文字块的版式

def layout_of(events, em_frac, honors_padding, honors_width, css_borders_outside_table):
    """[(文字, 版式)]、[(分页说明)]。版式：对齐、列宽（占内容区宽的比例，表格里的记 None）、每行几个字、
    上横线/下横线/左右竖线、前面有 <hr>、另起一页。`em_frac(em, 字号)`：盒子的 em 折成占页宽的比例。"""
    nodes, issues = [], []
    stack = []  # [(列宽, 盒子边框字母, 在表格里, 已有文字, 最后一个文字的下标)]
    avail = 1.0
    new_page, hr_before, sec_empty, rule_top = False, False, None, False

    def inner(b, avail, size):
        def lr(k):
            em, pct = b.get(k, (0.0, 0.0))
            return em_frac(em, size) + pct * avail
        w = avail
        if honors_width and b.get('width'):
            em, pct = b['width']
            w = min(avail, em_frac(em, size) + pct * avail)
        else:
            w -= lr('ml') + lr('mr')
        if honors_padding:
            w -= lr('pl') + lr('pr')
        return max(w, 0.0)

    for ev in events:
        kind = ev[0]
        if kind == 'sec':
            if sec_empty is not None and sec_empty:
                issues.append(('空白页', '一节里没有字也没有图'))
            if ev[1]:
                issues.append(('空白页', '文件开头的块上有强制分页'))
            new_page, sec_empty = True, True
        elif kind == 'open':
            _, tbl, b = ev
            cur = stack[-1][0] if stack else 1.0
            sides = b['sides'] if (tbl or css_borders_outside_table) else '----'
            stack.append([inner(b, cur, 1.0), sides, tbl, False, None, b['sides']])
        elif kind == 'close':
            col, sides, tbl, had, last, _ = stack.pop()
            if last is not None and 'B' in sides:
                nodes[last][1]['bottom'] = True
            if had and stack:
                stack[-1][3] = True
                stack[-1][4] = last
        elif kind == 'hr':
            hr_before = True
        elif kind == 'rule':
            # 成块的横线图：同一个盒子里前面已经有字的是上一块的下横线，否则是下一块的上横线。照着画的那条边框看不出来的
            # （颜色和底色差不多）横线图也看不出来，不算
            if not any(c in s[5] for s in stack for c in 'TB'):
                continue
            if nodes and (not stack or stack[-1][3]):
                nodes[-1][1]['bottom'] = True
            else:
                rule_top = True
        elif kind == 'img':
            sec_empty = False
            new_page = False
        elif kind == 'text':
            _, text, feat, b, rules = ev
            sec_empty = False
            size = feat.get('size_root', 1.0)
            cur = stack[-1][0] if stack else 1.0
            in_table = any(s[2] for s in stack)
            col = None if in_table else inner(b, cur, size)
            own = b.get('sides', '----') if (in_table or css_borders_outside_table) else '----'
            seen_tb = lambda c: c in b.get('sides', '') or any(c in s[5] for s in stack)
            rules = [r for r in rules if seen_tb('T' if r == 'top' else 'B')]
            top = rule_top or 'T' in own or 'top' in rules or any('T' in s[1] and not s[3] for s in stack)
            side = any(c in own for c in 'LR') or any(c in s[1] for s in stack for c in 'LR')
            lay = {'align': feat['align'], 'col': col, 'mt': feat.get('mt', 0.0), 'size': size, 'top': top, 'bottom': 'B' in own or 'bottom' in rules,
                   'side': side, 'hr': hr_before, 'page': new_page, 'len': text_len(KEY(text)), 'full': len(KEY(text)) < 60}
            nodes.append((text, lay))
            for s in stack:
                s[3] = True
                s[4] = len(nodes) - 1
            new_page = hr_before = rule_top = False
    return nodes, issues


def estimated_breaks(epub):
    """`wash/break_margin.rs` 照 Kindle 估算插的分页（类 `eink-pb`）开头的内容：(第一段字的前 60 个字, 开头的图路径)。Kindle 上这些地方
    是版面自己挪页，KFX 里不分节；掌阅、Move 照估算写成强制分页，会多出一节。版式检查把这些节并进前一节。"""
    import zipfile, html as H
    keys, imgs = set(), set()
    z = zipfile.ZipFile(epub)
    for name in z.namelist():
        if not name.lower().endswith(('.xhtml', '.html', '.htm')):
            continue
        t = z.read(name).decode('utf-8', 'replace')
        for m in re.finditer(r'<(\w+)\b[^>]*\bclass="[^"]*\beink-pb\b[^"]*"[^>]*>', t):
            tail = t[m.end():m.end() + 2000]
            first = re.search(r'<img\b[^>]*\bsrc="([^"]+)"', tail)
            txt = re.search(r'>([^<]*[^<\s][^<]*)<', '>' + tail) if not tail.lstrip().startswith('<img') else None
            if first and (not txt or tail.find(first.group(0)) < tail.find(txt.group(1))):
                imgs.add(posix_name(first.group(1)))
            elif txt:
                keys.add(KEY(H.unescape(txt.group(1))))
            elif first:
                imgs.add(posix_name(first.group(1)))
    return keys, imgs


def posix_name(src):
    return src.replace('\\', '/').rsplit('/', 1)[-1]


def section_kinds(events, soft=None):
    """每一节（一页的起点）的种类：`I` 只有图（封面、整页插图），`T` 有字，`E` 什么都没有；按阅读顺序。`soft`＝[`estimated_breaks`]
    的 (字, 图)：以它们开头的节并进前一节。"""
    kinds = []
    first, split = True, False
    for ev in events:
        if ev[0] == 'sec':
            kinds.append([0, 0])
            first = True
            split = len(ev) > 2 and not ev[2]  # 文件中间切出来的节（强制分页处）
            continue
        if not kinds or ev[0] not in ('text', 'img'):
            continue
        if first and soft and split and len(kinds) > 1:
            key = KEY(ev[1]) if ev[0] == 'text' else None
            if (ev[0] == 'text' and key in soft[0]) or (ev[0] == 'img' and len(ev) > 3 and posix_name(ev[3]) in soft[1]):
                prev = kinds.pop()
                kinds[-1][0] += prev[0]
                kinds[-1][1] += prev[1]
        first = False
        kinds[-1][0 if ev[0] == 'text' else 1] += 1
    return ['T' if t else 'I' if i else 'E' for t, i in kinds]


def cover_issues(k_kinds, d_kinds):
    """合集分书封面不多页（用户 2026-10-15）：只有图的节（封面、分书封面、整页插图）的个数和位置三台要一样；Kindle 没有的
    封面页、多出来的整页图都会让阅读器多翻一页。位置比的是「第几个只有图的节前面有几个有字的节」，不看绝对页码。"""
    def marks(kinds):
        out, texts = [], 0
        for k in kinds:
            if k == 'I':
                out.append(texts)
            elif k == 'T':
                texts += 1
        return out
    km, dm = marks(k_kinds), marks(d_kinds)
    found = []
    if len(km) != len(dm):
        found.append(('封面', f'只有图的页 Kindle {len(km)} 页、这台 {len(dm)} 页'))
    for i, (a, b) in enumerate(zip(km, dm)):
        if a != b:
            found.append(('封面', f'第 {i + 1} 个只有图的页：Kindle 前面有 {a} 页有字的、这台 {b} 页'))
            break
    return found


def per_line(lay, page_em):
    """一行放得下几个字（至少 1）。"""
    if lay['col'] is None:
        return None
    return max(1, math.floor(lay['col'] * page_em / lay['size'] + 1e-6))


# Kindle 内容区高（正文 rem）：1546px ÷ 一 rem 52.5px（profile kindle `line_px` 63 ÷ 1.2）
KINDLE_PAGE_REM = 1546 / 52.5


def compare(k_nodes, d_nodes, k_em, d_em, skip=frozenset()):
    """[(类别, 说明)]：Kindle 和这台每个配上的文字块逐项比。`skip`：这台上不比的文字（Move 搬进本章的注释）。"""
    q = collections.defaultdict(collections.deque)
    for t, lay in d_nodes:
        q[KEY(t)].append(lay)
    out = []
    # 连续几块同一处不一样（同一个框里的每一段都少了竖线）算一处：[类别, 说明, 块数]
    runs, prev = [], {}
    for t, k in k_nodes:
        key = KEY(t)
        if not key or not q[key]:
            continue
        d = q[key].popleft()
        if key in skip:
            continue
        ex = t.strip()[:20]
        out = []
        if {'left': 'left', 'justify': 'left'}.get(k['align'], k['align']) != {'left': 'left', 'justify': 'left'}.get(d['align'], d['align']):
            out.append(('对齐', f"Kindle {k['align']}，这台 {d['align']}：{ex}"))
        if k['col'] is not None and d['col'] is not None:
            kc, dc = per_line(k, k_em), per_line(d, d_em)
            narrow = lambda c: c <= 2
            if k['len'] >= 2 and narrow(kc) != narrow(dc) and min(kc, dc) < k['len']:
                out.append(('竖排', f'一行 Kindle {kc} 字、这台 {dc} 字：{ex}'))
            elif abs(k['col'] - d['col']) > 0.10 and not (k['full'] and k['len'] <= min(kc, dc)):
                out.append(('块宽', f"占页宽 Kindle {k['col']:.0%}、这台 {d['col']:.0%}：{ex}"))
        for f, name in (('top', '上横线'), ('bottom', '下横线'), ('side', '竖线'), ('hr', '前面的 <hr>')):
            if k[f] != d[f]:
                out.append(('横线', f"{name} Kindle {'有' if k[f] else '无'}、这台{'有' if d[f] else '无'}：{ex}"))
        # 上外边距占 Kindle 一页 1/4 以上的块在 Kindle 上放不下就挪到下一页（KINDLE_RULES 18 在两台按估算强制分页，`wash/break_margin.rs`）
        if k['page'] != d['page'] and not (d['page'] and k['mt'] >= 0.25 * KINDLE_PAGE_REM):
            out.append(('分页', f"{'Kindle 另起一页、这台没有' if k['page'] else '这台另起一页、Kindle 没有'}：{ex}"))
        cur = {}
        for c, msg in out:
            kind = msg.split('：')[0]
            if (c, kind) in prev:
                prev[(c, kind)][2] += 1
                cur[(c, kind)] = prev[(c, kind)]
            else:
                runs.append([c, msg, 1])
                cur[(c, kind)] = runs[-1]
        prev = cur
    return [(c, msg + (f'（连着 {n} 块）' if n > 1 else '')) for c, msg, n in runs]


def moved_notes(epub):
    """Move 搬进本章的注释（`<div class="footnotes">`，`htmlproc/footnote.rs`；用户定 Move 只认同文件锚点）的文字：Kindle 上注释照原书
    在别的文件，前面没有那条 <hr>、也不跟着章节分页，设计如此，不比。"""
    import zipfile, html
    z = zipfile.ZipFile(epub)
    keys = set()
    for name in z.namelist():
        if not name.lower().endswith(('.xhtml', '.html', '.htm')):
            continue
        s = z.read(name).decode('utf-8', 'replace')
        for m in re.finditer(r'<div class="footnotes">(.*?)</div>\s*(?:</body>|$)', s, re.S):
            for part in re.split(r'</(?:p|li|div|h[1-6])>', m.group(1)):
                t = html.unescape(re.sub(r'<[^>]+>', '', part))
                if KEY(t):
                    keys.add(KEY(t))
    return keys


def legacy_break_count(epub):
    """`break-before:page` 这类只有新写法、没有 `page-break-before` 的声明数（Move 不认）。"""
    import zipfile
    z = zipfile.ZipFile(epub)
    n = 0
    for name in z.namelist():
        if name.lower().endswith(('.css', '.xhtml', '.html', '.htm')):
            s = z.read(name).decode('utf-8', 'replace')
            n += len(re.findall(r'(?<![-\w])break-(?:before|after)\s*:\s*(?:page|always|left|right)', s))
    return n


def main():
    args = cli_args(2, __doc__)
    books, work, wanted = args[0], args[1], args[2:]
    opt = cli_opts()
    bindir = opt.get('--bin', 'target/release')
    toollib.BINDIR = bindir
    top = int(opt.get('--top', 8))
    reuse = '--reuse' in sys.argv
    base_path = opt.get('--baseline', os.path.join(os.path.dirname(os.path.abspath(__file__)), 'layout-baseline.tsv'))
    baseline = {}
    if os.path.exists(base_path):
        for line in open(base_path, encoding='utf-8'):
            if line.strip() and not line.startswith('#'):
                t, d, *v = line.rstrip('\n').split('\t')
                baseline[(t, d)] = dict(zip(CATS, map(int, v)))
    os.makedirs(work, exist_ok=True)
    page_em = {d: READER_GEOMETRY[d]['content'] / (READER_GEOMETRY[d]['em_px'] or float(profile_value(d, 'em_px'))) for d in DEVICES}
    seen, bad, total = {}, [], {d: collections.Counter() for d in DEVICES}
    examples = {d: collections.defaultdict(list) for d in DEVICES}
    print('== 每本：各台和 Kindle 版式不一样的处数（' + '、'.join(CATS) + '）')
    for src in books_in(books, wanted):
        title = os.path.splitext(os.path.basename(src))[0]
        stem = os.path.join(work, re.sub(r'[/\\\\]', '_', title))
        for d in ['kindle'] + DEVICES:
            if not (reuse and os.path.exists(f'{stem}.{d}.epub')):
                subprocess.run([f'{bindir}/epub-optimize', f'--device={d}', src, f'{stem}.{d}.epub'], check=True, capture_output=True)
        if not (reuse and os.path.exists(f'{stem}.kindle.kfx')):
            subprocess.run([f'{bindir}/epub-to-kfx', f'{stem}.kindle.epub', f'{stem}.kindle.kfx'], check=True, capture_output=True)
        base_fs, _ = epub_events(f'{stem}.kindle.epub', bindir, 'kindle')
        kev = kfx_events(f'{stem}.kindle.kfx', base_fs)
        for ev in kev:
            if ev[0] == 'text':
                ev[2]['size_root'] = ev[2]['size'] * base_fs
        k_nodes, _ = layout_of(kev, lambda em, size: em, True, True, True)
        k_kinds = section_kinds(kev)
        row = []
        for d in DEVICES:
            bfs, dev = epub_events(f'{stem}.{d}.epub', bindir, d)
            for ev in dev:
                if ev[0] == 'text':
                    ev[2]['size_root'] = ev[2]['fs']
            frac = lambda em, size, d=d: em / page_em[d]
            d_nodes, issues = layout_of(dev, frac, profile_flag(d, 'padding'), profile_flag(d, 'block_width'), profile_flag(d, 'borders'))
            skip = moved_notes(f'{stem}.{d}.epub') if profile_flag(d, 'repair_note_links') else frozenset()
            found = compare(k_nodes, d_nodes, PAGE, page_em[d], skip) + issues + cover_issues(k_kinds, section_kinds(dev, estimated_breaks(f'{stem}.{d}.epub')))
            if d == 'xochitl' and (c := legacy_break_count(f'{stem}.{d}.epub')):
                found.append(('怪癖', f'{c} 处 break-before/after:page（Move 只认 page-break-*）'))
            cnt = collections.Counter(c for c, _ in found)
            for c, msg in found:
                if len(examples[d][c]) < top * 4:
                    examples[d][c].append(f'{title}：{msg}')
            total[d].update(cnt)
            seen[(title, d)] = {c: cnt[c] for c in CATS}
            known = baseline.get((title, d))
            worse = any(cnt[c] > (known or {}).get(c, 0) for c in CATS)
            if worse:
                bad.append(f'{title}（{d}）')
            row.append(f"{d} " + '/'.join(str(cnt[c]) for c in CATS) + (' ✗' if worse else ''))
        print(f'  {title}: ' + '  '.join(row))
    for d in DEVICES:
        print(f'== {d}：' + '  '.join(f'{c} {total[d][c]}' for c in CATS))
        for c in CATS:
            for e in examples[d][c][:top]:
                print(f'  [{c}] {e}')
    if '--update' in sys.argv:
        baseline.update(seen)
        with open(base_path, 'w', encoding='utf-8') as f:
            f.write('# tools/kfx/layout.py 的基线：书名\t阅读模式\t' + '\t'.join(CATS) + '（处数）。--update 写回\n')
            for (t, d) in sorted(baseline):
                f.write(f'{t}\t{d}\t' + '\t'.join(str(baseline[(t, d)].get(c, 0)) for c in CATS) + '\n')
        print(f'已写回基线 {base_path}')
        return
    if bad:
        print('✗ 比基线多出版式差异：' + '、'.join(bad))
        sys.exit(1)
    print('✓ 没有比基线多出的版式差异')


if __name__ == '__main__':
    main()
