"""实际显示抽样：同一本书同一处（章首）在 Kindle、掌阅、Move 上排出来的样子，按像素量出每行字的位置再比。
unify.py 比的是交给阅读器的样式（阅读器自己的怪癖不在其中）；这里比屏幕上真的画出来的（2026-10-14 P0，维护者要的）。

用法：
  python3 sample.py move    书.epub 输出目录 --title=章名 [--host=root@10.11.99.1] [--pages=2]
      经书架服务导入 Move（进根目录，名字「测试-抽样-…」，用完请删），取回 xochitl 排的 PDF，按文字找到章名所在页，渲染成屏幕像素的 PNG
      （PDF 的字是康熙部首这类兼容字符，按 NFKC 归一以后找）。
  python3 sample.py ireader 书.epub 输出目录 --title=章名 [--pages=2] [--serial=…]
      adb 推进掌阅 documents/测试/（用完请删），文件管理器里点开，菜单键 → 目录 →（展开全部）→ 点章名，逐页截屏。
  python3 sample.py kindle  输出目录 [--last=1]
      从 Kindle（jmtpfs 挂载）的 screenshots/ 拷最新的几张截图（Kindle 没越狱，截图得维护者在真机上截）。
  python3 sample.py measure 图...
      量每行字：(上, 下, 左, 右) 像素，正文行距，文字区左右边界。
  python3 sample.py compare 参照图 图...
      都按各自的正文行距、文字区宽度归一，按块（段落、标题、图）比每块开头那一行：和上一块中线的距离（行）、左边缩进（文字区宽的
      %）、字高（正文字高的倍数）。距离差 0.25 行以上、缩进差 1.5% 以上、字高差 10% 以上的标出来。参照一般是 Kindle 的截图。

各台裁掉的状态栏、页码：Kindle 截图上下各 75px（阅读范围 1546 高），掌阅底部 80px；Move 的 PDF 只有正文。
注意：「行」是各台自己的正文行距。Kindle 的上下边距按行距的倍数存（跟着「Aa → 行距」设置变），掌阅、Move 的按字号算（不跟行距变）；
三台的「行距 ÷ 字高」不一样时（2026-10-14《人生海海》：Kindle 1.66、Move 1.44、掌阅 1.51），以行算的距离里有一部分是设置的差别，
不全是排版规则的差别。"""
import json, os, re, statistics, subprocess, sys, time, unicodedata, urllib.parse, urllib.request
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))
from toollib import cli_args, cli_opts

KINDLE_SHOTS = '/run/user/1000/mtp/kindle/Internal Storage/screenshots'
XOCHITL = '/home/root/.local/share/remarkable/xochitl'
MOVE_WIDTH = 954  # Move 屏幕宽（PDF 按它渲染，1px＝屏幕 1px）


def norm(s):
    return re.sub(r'\s+', '', unicodedata.normalize('NFKC', s))


# ---------------------------------------------------------------- Move

def move(book, out, title, host, pages):
    name = f"测试-抽样-{os.path.splitext(os.path.basename(book))[0]}-{time.strftime('%H%M%S')}.epub"
    tunnel = subprocess.Popen(['ssh', '-o', 'ControlMaster=no', '-o', 'ExitOnForwardFailure=yes', '-N', '-L', '8790:127.0.0.1:8790', host])
    try:
        time.sleep(2)
        req = urllib.request.Request('http://127.0.0.1:8790/import?name=' + urllib.parse.quote(name) + '&folder=', data=open(book, 'rb').read(),
                                     headers={'Content-Type': 'application/epub+zip'}, method='POST')
        v = json.load(urllib.request.urlopen(req, timeout=180))
        while 'job' in v and v.get('state') != 'done':
            time.sleep(2)
            v = json.load(urllib.request.urlopen(f"http://127.0.0.1:8790/import/jobs/{v['job']}", timeout=30))
            if v.get('state') == 'failed':
                sys.exit(f'导入失败：{v}')
        uid = v['uuid']
    finally:
        tunnel.terminate()
    pdf = os.path.join(out, 'move.pdf')
    for _ in range(90):  # 大书排版要一会儿
        if subprocess.run(['scp', '-q', f'{host}:{XOCHITL}/{uid}.pdf', pdf]).returncode == 0:
            break
        time.sleep(5)
    else:
        sys.exit('等不到 Move 排好的 PDF')
    n = int(re.search(r'Pages:\s+(\d+)', subprocess.run(['pdfinfo', pdf], capture_output=True, text=True).stdout).group(1))
    want = norm(title)
    texts = {p: [norm(l) for l in subprocess.run(['pdftotext', '-f', str(p), '-l', str(p), '-layout', pdf, '-'], capture_output=True, text=True).stdout.splitlines() if norm(l)]
             for p in range(1, n + 1)}
    # 章首：章名是这一页的第一行（目录页里也有单独成行的章名）；没有的话取第一个单独成行的
    page = next((p for p, ls in texts.items() if ls[:1] == [want]), None) or next((p for p, ls in texts.items() if want in ls), None)
    if page is None:
        sys.exit(f'PDF 里没找到单独成行的「{title}」')
    shots = []
    for k in range(pages):
        base = os.path.join(out, f'move-{k + 1}')
        subprocess.run(['pdftoppm', '-f', str(page + k), '-l', str(page + k), '-scale-to-x', str(MOVE_WIDTH), '-scale-to-y', '-1', '-gray', '-png', '-singlefile', pdf, base], check=True)
        shots.append(base + '.png')
    print(f'Move：「{title}」在第 {page} 页；{" ".join(shots)}；测试书「{name}」在 Move 根目录，用完请删')


# ---------------------------------------------------------------- 掌阅

def adb(serial, *a, out=None):
    r = subprocess.run(['adb', *(['-s', serial] if serial else []), *a], capture_output=True)
    if out:
        with open(out, 'wb') as f:
            f.write(r.stdout)
    return r.stdout.decode('utf-8', 'replace')


def ui(serial):
    adb(serial, 'shell', 'uiautomator', 'dump', '/sdcard/ui.xml')
    return [(t, tuple(map(int, b))) for t, *b in re.findall(r'text="([^"]*)"[^>]*bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"', adb(serial, 'shell', 'cat', '/sdcard/ui.xml'))]


def tap(serial, pred):
    for t, (x1, y1, x2, y2) in ui(serial):
        if t and pred(t):
            adb(serial, 'shell', 'input', 'tap', str((x1 + x2) // 2), str((y1 + y2) // 2))
            time.sleep(2)
            return True
    return False


def ireader(book, out, title, pages, serial):
    name = f"测试-抽样-{time.strftime('%m%d%H%M%S')}"
    adb(serial, 'shell', 'input', 'keyevent', 'KEYCODE_WAKEUP')
    adb(serial, 'shell', 'mkdir', '-p', '/sdcard/documents/测试')
    adb(serial, 'push', book, f'/sdcard/documents/测试/{name}.epub')
    adb(serial, 'shell', 'am', 'start', '-n', 'com.android.documentsui/com.szzy.ireader.ink.fileexplorer.ui.activity.MainActivity')
    time.sleep(3)
    for _ in range(6):
        if tap(serial, lambda t: name in t):
            break
        if not tap(serial, lambda t: t == '测试') and not tap(serial, lambda t: t == 'documents'):
            tap(serial, lambda t: '存储空间' in t)
    else:
        sys.exit(f'文件管理器里没找到「{name}」')
    time.sleep(8)
    want = norm(title)
    # 菜单键 → 目录；目录是分级折叠的，找不到就「展开全部」，再往下翻几页
    adb(serial, 'shell', 'input', 'keyevent', 'KEYCODE_MENU')
    time.sleep(2)
    if not tap(serial, lambda t: t == '目录'):
        sys.exit('掌阅菜单里没有「目录」')
    tap(serial, lambda t: t == '展开全部')
    for _ in range(30):
        if tap(serial, lambda t: norm(t) == want):
            break
        adb(serial, 'shell', 'input', 'swipe', '632', '1300', '632', '500', '300')
        time.sleep(1.5)
    else:
        sys.exit(f'掌阅目录里没找到「{title}」')
    time.sleep(8)  # 跳转后底部有「返回至第 N 页」的提示，等它消失
    shots = []
    for k in range(pages):
        png = os.path.join(out, f'ireader-{k + 1}.png')
        adb(serial, 'exec-out', 'screencap', '-p', out=png)
        shots.append(png)
        adb(serial, 'shell', 'input', 'swipe', '1100', '1580', '150', '1580', '200')
        time.sleep(2.5)
    print(f'掌阅：{" ".join(shots)}；测试书 documents/测试/{name}.epub 用完请删')


# ---------------------------------------------------------------- Kindle

def kindle(out, last):
    names = sorted(f for f in os.listdir(KINDLE_SHOTS) if f.endswith('.png'))[-last:]
    shots = []
    for k, f in enumerate(names):
        dst = os.path.join(out, f'kindle-{k + 1}.png')
        subprocess.run(['cp', os.path.join(KINDLE_SHOTS, f), dst], check=True)
        shots.append(dst)
    print('Kindle：' + ' '.join(shots))


# ---------------------------------------------------------------- 测量、比较

def crop_of(path, w, h):
    """各台的正文范围（上, 下）：按截图大小认是哪台。"""
    if (w, h) == (1272, 1696):  # Kindle 截图
        return 75, h - 75
    if (w, h) == (1264, 1680):  # 掌阅截屏：底部页码、时间
        return 0, h - 80
    return 0, h


def measure(path):
    from PIL import Image
    im = Image.open(path).convert('L')
    w, h = im.size
    top, bottom = crop_of(path, w, h)
    px = im.load()
    dark = lambda x, y: px[x, y] < 128
    rows = [sum(1 for x in range(0, w, 2) if dark(x, y)) for y in range(h)]
    runs, y = [], top
    while y < bottom:
        if rows[y]:
            s = y
            while y < bottom and rows[y]:
                y += 1
            runs.append((s, y))
        y += 1
    lines = []
    for a, b in runs:
        if b - a < 4:
            continue
        xs = [x for x in range(w) for yy in range(a, b, 2) if dark(x, yy)]
        lines.append({'top': a, 'bottom': b, 'left': min(xs), 'right': max(xs)})
    hs = sorted(l['bottom'] - l['top'] for l in lines)
    body_h = statistics.median(hs) if hs else 0
    body = [l for l in lines if abs((l['bottom'] - l['top']) - body_h) <= max(3, body_h * 0.2)]
    gaps = [(b['top'] + b['bottom'] - a['top'] - a['bottom']) / 2 for a, b in zip(body, body[1:])]
    pitch = statistics.median(gaps) if gaps else 0
    left = min((l['left'] for l in body), default=0)
    right = max((l['right'] for l in body), default=w)
    for l in lines:
        l['kind'] = 'image' if pitch and l['bottom'] - l['top'] > 2.5 * pitch else 'text'
    return {'path': path, 'size': [w, h], 'pitch': pitch, 'glyph': body_h, 'text_left': left, 'text_right': right, 'lines': lines}


def normalized(m):
    p, width = m['pitch'] or 1, (m['text_right'] - m['text_left']) or 1
    out = []
    for l in m['lines']:
        out.append({'center': (l['top'] + l['bottom']) / 2 / p, 'indent': (l['left'] - m['text_left']) / width * 100,
                    # 按字算的缩进：中文字的墨迹高约 0.9 个字宽，正文字高当一个字
                    'indent_em': (l['left'] - m['text_left']) / (m['glyph'] or 1),
                    'height': (l['bottom'] - l['top']) / (m['glyph'] or 1), 'kind': l['kind']})
    return out


def blocks(m):
    """一页的行按块分开：和上一行空得比 1.3 行多、或者出现首行缩进（缩进 ≥ 3%、上一行没缩进）就是新的一块。各台每行字数不同，
    段落中间换行的位置不一样，只比每块开头那一行。"""
    rows = normalized(m)
    out = []
    for i, r in enumerate(rows):
        gap = r['center'] - rows[i - 1]['center'] if i else 0.0
        new = i == 0 or gap > 1.3 or r['kind'] == 'image' or rows[i - 1]['kind'] == 'image' or (r['indent'] >= 3 and rows[i - 1]['indent'] < 1)
        if new:
            out.append({'gap': gap, 'indent': r['indent'], 'indent_em': r['indent_em'], 'height': r['height'], 'kind': r['kind'], 'lines': 1})
        else:
            out[-1]['lines'] += 1
    return out


def compare(paths):
    ms = [measure(p) for p in paths]
    bs = [blocks(m) for m in ms]
    n = min(len(x) for x in bs)
    print('参照：' + paths[0] + '；正文行距 ' + '、'.join(f"{os.path.basename(m['path'])} {m['pitch']:.1f}px" for m in ms))
    print('块  ' + ''.join(f'{os.path.basename(p)[:24]:>42}' for p in paths))
    print('    ' + ''.join(f"{'距上块(行) 缩进% 缩进字 字高 行数':>42}" for _ in paths))
    flags = 0
    for i in range(n):
        cells = []
        r = bs[0][i]
        for k, x in enumerate(bs):
            b = x[i]
            cell = f"{b['gap']:7.2f} {b['indent']:6.1f} {b['indent_em']:5.1f} {b['height']:5.2f} {b['lines']:3d}{'图' if b['kind'] == 'image' else '  '}"
            if k:
                # 每块的行数不比（各台一行的字数不同）；块里最后一行到下一块的距离也按块开头算
                # 缩进：Kindle 上首行缩进按字（em）、左右边距按页宽（%），分不清是哪种，两种都对不上才算
                indent_bad = abs(b['indent'] - r['indent']) > 1.5 and abs(b['indent_em'] - r['indent_em']) > 0.3
                bad = abs(b['gap'] - r['gap']) > 0.25 or indent_bad or abs(b['height'] - r['height']) > 0.1 * r['height']
                flags += bad
                cell += ' ✗' if bad else '  '
            cells.append(f'{cell:>42}')
        print(f'{i + 1:<4}' + ''.join(cells))
    if any(len(x) != n for x in bs):
        print('（块数不一样：' + '、'.join(str(len(x)) for x in bs) + '，只比前 ' + str(n) + ' 块；页尾的块可能被截断）')
    print(f'标出 {flags} 处（距离差 > 0.25 行、缩进差 > 1.5% 而且 > 0.3 字、字高差 > 10%）')


def main():
    pos = cli_args(1, __doc__)
    opt = cli_opts()
    cmd = pos[0]
    if cmd in ('move', 'ireader'):
        if len(pos) < 3 or '--title' not in opt:
            sys.exit(__doc__)
        os.makedirs(pos[2], exist_ok=True)
        pages = int(opt.get('--pages', 2))
        if cmd == 'move':
            move(pos[1], pos[2], opt['--title'], opt.get('--host', 'root@10.11.99.1'), pages)
        else:
            ireader(pos[1], pos[2], opt['--title'], pages, opt.get('--serial'))
    elif cmd == 'kindle' and len(pos) >= 2:
        os.makedirs(pos[1], exist_ok=True)
        kindle(pos[1], int(opt.get('--last', 1)))
    elif cmd == 'measure' and len(pos) >= 2:
        for p in pos[1:]:
            print(json.dumps(measure(p), ensure_ascii=False))
    elif cmd == 'compare' and len(pos) >= 3:
        compare(pos[1:])
    else:
        sys.exit(__doc__)


if __name__ == '__main__':
    main()
