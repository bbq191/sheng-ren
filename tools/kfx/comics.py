"""三台漫画版式检查：每一整页图在屏幕上是不是不拉伸、居中、受限的那条边离屏幕边 1–2px，页数和原书一样。
用法：python3 comics.py <书目录或书> <工作目录> [--bin=target/release] [--reuse] [--top=8] [书名子串…]

2026-10-15 用户定的重点之一（「漫画能不拉伸贴边居中留 1-2px」）。只看产物本身（黑盒），不调优化器的排版函数：
- 原书、产物按阅读顺序各取每页的第一张图配对；页数（有图的页）不一样直接报；
- 整页图（产物短边不小于阅读范围的 1/3；装饰小图、纯文字页不看）按阅读器的显示方式放到屏幕上：图按比例缩进漫画的阅读范围
  （profile `[comic_readable]`，没有的用 `[readable.epub]`），范围在屏幕上的位置——Kindle、掌阅是整屏，Move 页边距设成 1 以后
  在 (1, 112)（profile 注释，2026-09-29 真机）；
- 不拉伸：整页图（画布）和阅读范围同比例（差 1px 以内）——阅读器把它缩放进范围；
- 画面（灰度低于 235 的外框）在阅读范围里左右、上下各自居中（差 3px 以内，画面边上很淡的像素缩放后会变白）；受限的那条边（离范围边最近的）离屏幕边 1–2px，Move 上下受限时屏幕上下
  留的 112/127px 改不了，只要求离范围边 1px 以内；
- Kindle 另查 KFX 固定版式：画布 = 屏幕，图片节点宽高 = 图的像素（1:1），位置居中。像素用转 KFX 前的 EPUB（写出器原样放进去）。"""
import collections, io, os, re, subprocess, sys, zipfile
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kfx import by_type, cli_args, cli_opts, opf_path, reading_order
from PIL import Image

PROFILES = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'crates', 'profile', 'profiles')
DEVICES = ['kindle', 'ireader', 'xochitl']
# 漫画的阅读范围在屏幕上的左上角（像素）
AREA_ORIGIN = {'kindle': (0, 0), 'ireader': (0, 0), 'xochitl': (1, 112)}
HARD = {'页数', '拉伸', '翻页', 'KFX'}


def geometry(device):
    text = open(os.path.join(PROFILES, f'{device}.toml'), encoding='utf-8').read()
    sec = lambda name: tuple(map(int, re.search(r'\[' + re.escape(name) + r'\]\s*width\s*=\s*(\d+)\s*height\s*=\s*(\d+)', text).groups())) if re.search(r'\[' + re.escape(name) + r'\]', text) else None
    return {'screen': sec('screen'), 'area': sec('comic_readable') or sec('readable.epub')}


def page_images(path):
    """[(书内路径, 字节)]：按 spine 顺序每页第一张图（`<img src>`、SVG `<image href>`）；没有图的页跳过。"""
    z = zipfile.ZipFile(path)
    opf = opf_path(z)
    base = os.path.dirname(opf)
    o = z.read(opf).decode('utf-8', 'replace')
    items = {m.group(1): m.group(2) for m in re.finditer(r'<item\b[^>]*?\bid=["\']([^"\']+)["\'][^>]*?\bhref=["\']([^"\']+)["\']', o)}
    items.update({m.group(2): m.group(1) for m in re.finditer(r'<item\b[^>]*?\bhref=["\']([^"\']+)["\'][^>]*?\bid=["\']([^"\']+)["\']', o)})
    out = []
    for idref in re.findall(r'<itemref\b[^>]*?\bidref=["\']([^"\']+)["\']', o):
        href = items.get(idref)
        if not href:
            continue
        p = os.path.normpath(os.path.join(base, href)).replace('\\', '/')
        try:
            page = z.read(p).decode('utf-8', 'replace')
        except KeyError:
            continue
        m = re.search(r'<(?:img|image)\b[^>]*?\b(?:src|xlink:href|href)=["\']([^"\']+)["\']', page)
        if not m:
            continue
        ip = os.path.normpath(os.path.join(os.path.dirname(p), m.group(1).split('#')[0])).replace('\\', '/')
        try:
            out.append((ip, z.read(ip)))
        except KeyError:
            pass
    return out


def spine_direction(path):
    """OPF spine 的 `page-progression-direction`（没写 `default`）。"""
    z = zipfile.ZipFile(path)
    m = re.search(r'<spine\b[^>]*?\bpage-progression-direction=["\']([^"\']+)', z.read(opf_path(z)).decode('utf-8', 'replace'))
    return m.group(1) if m else 'default'


def kfx_direction(path):
    """KFX 固定版式页上写的翻页方向（`$560`）：`$557` 从左往右、`$559` 从右往左（`crates/kfx/src/yj.rs` 的 `DIR_LTR`/`DIR_RTL`）。"""
    E = by_type(path)
    for s in reading_order(E):
        for t in E['$260'][s]['$141']:
            if '$560' in t:
                return {'$557': 'ltr', '$559': 'rtl'}.get(t['$560'], t['$560'])
    return 'default'


def ink_box(img):
    """产物里画的外框 (左, 上, 右, 下)（右、下不含）：有灰度低于 235 的像素的行、列。漫画页是贴在纯白画布上的（`imgopt`
    `paste_on_white`），画布上只有 JPEG 振铃（量到过 238–254，所以门槛不能再高）。画面边上整行很淡的（《哆啦A夢》有的页顶上一截
    纸色 247–255）量不进来，会报成留边过宽，要看图确认。不用来量原书：原书的纸色、扫描边和画面分不开。"""
    return img.convert('L').point(lambda v: 255 if v < 235 else 0).getbbox()


def kfx_fixed_pages(path, screen):
    """[(说明)]：KFX 固定版式页不对的地方。"""
    E = by_type(path)
    styles = E.get('$157', {})
    res = {v['$175']: v for v in E.get('$164', {}).values() if isinstance(v, dict) and '$175' in v}
    bad = []
    num = lambda v: float(v['$307']) if isinstance(v, dict) else float(v)
    for s in reading_order(E):
        for t in E['$260'][s]['$141']:
            if '$66' not in t:
                continue
            cw, ch = t['$66'], t['$67']
            if (cw, ch) != screen:
                bad.append(f'画布 {cw}×{ch}，屏幕 {screen[0]}×{screen[1]}')
            sl = E['$259'].get(t.get('$176'), {})
            for n in sl.get('$146', []):
                kids = n.get('$146', []) if isinstance(n, dict) else []
                for k in kids or [n]:
                    if not isinstance(k, dict) or k.get('$159') != '$271':
                        continue
                    st = styles.get(str(k.get('$157')), {})
                    meta = res.get(k.get('$175'), {})
                    w, h = num(st.get('$56', 0)), num(st.get('$57', 0))
                    if (round(w), round(h)) != (meta.get('$422'), meta.get('$423')):
                        bad.append(f"图片节点 {w:.0f}×{h:.0f}、图 {meta.get('$422')}×{meta.get('$423')}（不是 1:1）")
                    if '$58' in st and (abs(num(st['$58']) - (ch - h) / 2) > 1 or abs(num(st['$59']) - (cw - w) / 2) > 1):
                        bad.append(f"图片位置 ({num(st['$59']):.0f}, {num(st['$58']):.0f}) 不居中")
    return bad


def check_pages(src_pages, out_pages, device, geo):
    """[(类别, 说明)]、量到的受限边离屏幕的距离 Counter。"""
    found, dist = [], collections.Counter()
    if len(src_pages) != len(out_pages):
        found.append(('页数', f'原书 {len(src_pages)} 页有图、产物 {len(out_pages)} 页'))
    aw, ah = geo['area']
    sw, sh = geo['screen']
    ox, oy = AREA_ORIGIN[device]
    for i, ((sp, sb), (op, ob)) in enumerate(zip(src_pages, out_pages)):
        try:
            oim, sim = Image.open(io.BytesIO(ob)), Image.open(io.BytesIO(sb))
        except Exception:
            continue
        iw, ih = oim.size
        if min(iw, ih) < aw / 3:
            continue
        obox = ink_box(oim)
        if not obox:
            continue
        where = f'第 {i + 1} 页 {os.path.basename(op)}'
        # 不拉伸：画布和阅读范围同比例（阅读器把整页图缩放到范围里，比例不同就会被压扁或拉长；`comicfxl::fills_canvas` 同样的容差），
        # 画面本身优化器按原比例缩放（`imgopt::comic_layout`），原书的纸边分不开，不逐像素比
        if abs(iw * ah - ih * aw) > max(aw, ah):
            found.append(('拉伸', f'{where}：图 {iw}×{ih}，阅读范围 {aw}×{ah}'))
        # 放到屏幕上：图按比例缩进阅读范围，居中
        scale = min(aw / iw, ah / ih)
        dw, dh = iw * scale, ih * scale
        x0, y0 = ox + (aw - dw) / 2, oy + (ah - dh) / 2
        L, T = x0 + obox[0] * scale, y0 + obox[1] * scale
        R, B = x0 + obox[2] * scale, y0 + obox[3] * scale
        l, r, t, b = L - ox, ox + aw - R, T - oy, oy + ah - B
        if abs(l - r) > 3 or abs(t - b) > 3:
            found.append(('居中', f'{where}：离范围左右 {l:.0f}/{r:.0f}、上下 {t:.0f}/{b:.0f}'))
        if min(l, r) <= min(t, b):
            edge = (L, sw - R)
            d = round(min(edge))
            dist[f'左右 {d}'] += 1
            if not all(1 <= round(e) <= 2 for e in edge):
                found.append(('留边', f'{where}：左右受限，离屏幕 {edge[0]:.0f}/{edge[1]:.0f}px'))
        else:
            edge = (T, sh - B)
            d = round(min(edge))
            dist[f'上下 {d}'] += 1
            ok = all(1 <= round(e) <= 2 for e in edge) if (ox, oy) == (0, 0) else max(t, b) <= 1.5
            if not ok:
                found.append(('留边', f'{where}：上下受限，离屏幕 {edge[0]:.0f}/{edge[1]:.0f}px'))
    return found, dist


def profile_str(device, key):
    for line in open(os.path.join(PROFILES, f'{device}.toml'), encoding='utf-8'):
        k, _, v = line.partition('=')
        if k.strip() == key:
            return v.split('#')[0].strip().strip('"')
    return None


def comic_books(root, wanted):
    if os.path.isfile(root):
        return [root]
    out = []
    for d, _, files in os.walk(root):
        for f in files:
            p = os.path.join(d, f)
            if f.lower().endswith(('.epub', '.cbz')) and '漫画' in p and (not wanted or any(w in f for w in wanted)):
                out.append(p)
    return sorted(out)


def main():
    args = cli_args(2, __doc__)
    books, work, wanted = args[0], args[1], args[2:]
    opt = cli_opts()
    bindir = opt.get('--bin', 'target/release')
    top = int(opt.get('--top', 8))
    reuse = '--reuse' in sys.argv
    os.makedirs(work, exist_ok=True)
    geo = {d: geometry(d) for d in DEVICES}
    total = {d: collections.Counter() for d in DEVICES}
    dists = {d: collections.Counter() for d in DEVICES}
    examples = {d: collections.defaultdict(list) for d in DEVICES}
    any_bad = False
    notes = set()
    for src in comic_books(books, wanted):
        title = os.path.splitext(os.path.basename(src))[0]
        stem = os.path.join(work, re.sub(r'[/\\\\]', '_', title))
        if src.lower().endswith('.cbz'):
            continue  # CBZ 原书没有 spine，先不比
        src_pages = page_images(src)
        row = []
        for d in DEVICES:
            out = f'{stem}.{d}.epub'
            if not (reuse and os.path.exists(out)):
                subprocess.run([f'{bindir}/epub-optimize', f'--device={d}', src, out], check=True, capture_output=True)
            found, dist = check_pages(src_pages, page_images(out), d, geo[d])
            if d == 'kindle':
                kfx = f'{stem}.kindle.kfx'
                if not (reuse and os.path.exists(kfx)):
                    subprocess.run([f'{bindir}/epub-to-kfx', out, kfx], check=True, capture_output=True)
                found += [('KFX', m) for m in kfx_fixed_pages(kfx, geo[d]['screen'])]
            # 翻页方向照书（2026-10-15 用户）：profile 写了 `comic_page_direction` 的（掌阅 ltr，2026-09-30 用户为铺满整屏定的）照写的算
            want = profile_str(d, 'comic_page_direction') or spine_direction(src)
            got = kfx_direction(f'{stem}.kindle.kfx') if d == 'kindle' else spine_direction(out)
            if want != 'default' and got != want:
                found.append(('翻页', f'书里 {spine_direction(src)}、产物 {got}'))
            elif profile_str(d, 'comic_page_direction') and spine_direction(src) not in ('default', got):
                notes.add(f'{d}：profile 定成 {got}，书里 {spine_direction(src)}')
            cnt = collections.Counter(c for c, _ in found)
            total[d].update(cnt)
            dists[d].update(dist)
            for c, m in found:
                if len(examples[d][c]) < top:
                    examples[d][c].append(f'{title}：{m}')
            # 居中、留边是从像素量的（JPEG 振铃、纸色淡边会差几 px），列出来要看图；页数、拉伸、翻页、KFX 结构才算不过
            any_bad |= any(c in HARD for c, _ in found)
            row.append(f'{d} ' + ('✓' if not found else ' '.join(f'{c} {n}' for c, n in cnt.items())))
        print(f'  {title}（{len(src_pages)} 页）: ' + '  '.join(row))
    for d in DEVICES:
        print(f'== {d}：受限边离屏幕的距离（页数）' + '  '.join(f'{k}px {n}' for k, n in sorted(dists[d].items())))
        for c, lst in examples[d].items():
            for e in lst:
                print(f'  [{c}] {e}')
    print('（居中、留边按像素量，原书纸色偏灰、画面边上很淡的页会多报，要看图确认；不影响退出码）')
    for n in sorted(notes):
        print(f'  （翻页方向没照书：{n}）')
    if any_bad:
        sys.exit(1)
    print('✓ 页数、不拉伸、翻页方向、KFX 固定版式都对（居中、留边看上面的列表）')


if __name__ == '__main__':
    main()
