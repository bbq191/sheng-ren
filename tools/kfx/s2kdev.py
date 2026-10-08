"""三台（Kindle 的 KFX、掌阅和 Move 的 EPUB）和 Send to Kindle 的一致性：逐个文字节点、逐项比阅读器看到的样式。
用法：python3 s2kdev.py <Amazon KFX 目录> <书目录> <工作目录> [--bin=target/release] [--top=12]

每本书用 epub-optimize 生成 kindle / ireader / xochitl 三份产物（书目录只读），Kindle 再转 KFX；掌阅、Move 的 EPUB 用
`epub-to-kfx --styles` 按 CSS 原样算出每个文字块的样式（阅读器自己的怪癖不在其中，见 docs/kfx.md#三台和-send-to-kindle-的一致性）。
都折算成和阅读器字号设置无关的量再比：
  字号、行高 = 相对正文的倍数；上下边距 = 正文 em；首行缩进 = 本元素 em；左右边距 = 页宽 %（整页 32em）；
  颜色（没写＝黑）、字体（阅读器字体记作 reader）、粗细（粗/正常）、斜体、对齐。
数值相差 3% 以内或 0.02 以内算一致。"""
import collections, os, re, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kfx import cli_args, cli_opts
import s2kcmp
from s2kbatch import book_index, find_source

PAGE = 32.0
FEATURES = ['size', 'lh', 'mt', 'mb', 'indent', 'ml', 'mr', 'color', 'font', 'weight', 'italic', 'align']
TEXT_PROPS = ['size', 'line_h', 'font', 'color', 'weight', 'style', 'align', 'indent']
ALIGN = {"'$320'": 'center', "'$61'": 'right', "'$59'": 'left', "'$321'": 'justify'}


def num(v):
    m = re.match(r'(-?[\d.]+)', v or '')
    return float(m.group(1)) if m else None


def kfx_features(chain, base_fs):
    """KFX 节点（外层容器 → 文字）的样式链 → 特征。文字属性从里往外找；文字自己没有边距（只写了对齐的）时用最近容器的。"""
    leaf = chain[-1]
    def find(k):
        for d in reversed(chain):
            if k in d:
                return d[k]
        return None
    size = num(find('size')) or 1.0
    lh = num(find('line_h')) or 1.0
    box = leaf
    if not any(k in leaf for k in ('m_top', 'm_bottom', 'm_left', 'm_right')):
        box = next((d for d in reversed(chain[:-1]) if d), leaf)
    vert = lambda k: (num(box.get(k)) or 0.0) * 1.2 * lh * size
    def horiz(k):
        v = box.get(k)
        if not v:
            return 0.0
        return num(v) if v.endswith('%') else num(v) * size * base_fs / PAGE * 100
    ind = find('indent')
    if not ind:
        indent = 0.0
    elif ind.endswith('%'):
        indent = num(ind) / 100 * PAGE / (size * base_fs)
    else:
        indent = num(ind)
    font = (find('font') or '-').strip("'").split(',')[0]
    font = 'reader' if font in ('default', '-') else re.sub(r'^(j\d+-\d+-|v\d+-|section\d+-)', '', font.lower())
    color = find('color') or '#ff000000'
    w = find('weight') or "'$350'"
    return {'size': size, 'lh': lh * size, 'mt': vert('m_top'), 'mb': vert('m_bottom'), 'indent': indent, 'ml': horiz('m_left'), 'mr': horiz('m_right'),
            'color': color.lower(), 'font': font, 'weight': 'normal' if w == "'$350'" else 'bold', 'italic': find('style') == "'$382'",
            'align': ALIGN.get(find('align') or '', 'left')}


def epub_features(row, base_fs, base_lh):
    key, fs, lh, fam, weight, italic, color, align, indent, mt, mb, ml, mr = row
    fs, lh = float(fs), float(lh)
    if indent == '-':
        ind = 0.0
    else:
        u, v = indent.split(':')
        v = float(v)
        ind = v if u == 'em' else v / 12 / fs if u == 'pt' else v / 100 * PAGE / fs
    h = lambda s: (float(s.split('+')[0]) / PAGE * 100 + float(s.split('+')[1]))
    font = 'reader' if fam == '-' else fam.lower()
    return {'size': fs / base_fs, 'lh': lh / base_lh, 'mt': float(mt) / base_fs, 'mb': float(mb) / base_fs, 'indent': ind, 'ml': h(ml), 'mr': h(mr),
            'color': '#' + (color if color != '-' else 'ff000000'), 'font': font, 'weight': 'normal' if weight == 'normal' else 'bold',
            'italic': italic == 'true', 'align': {'-': 'left', 'start': 'left', 'end': 'right'}.get(align, align)}


def epub_nodes(path, bindir, device):
    out = subprocess.run([f'{bindir}/epub-to-kfx', '--styles', f'--device={device}', path], check=True, capture_output=True, text=True).stdout.splitlines()
    _, bfs, blh = out[0].split('\t')
    rows = [l.split('\t') for l in out[1:] if l.count('\t') == 12]
    return float(bfs), float(blh), rows


def same(f, a, b):
    if isinstance(a, float):
        return abs(a - b) <= max(0.02, 0.03 * max(abs(a), abs(b)))
    if f == 'align' and {a, b} <= {'left', 'justify'}:
        return True  # 左对齐和两端对齐的差别看不出来（中文没有词间空格）
    return a == b


def main():
    amz_dir, books, work = cli_args(3, __doc__)[:3]
    opt = cli_opts()
    bindir = opt.get('--bin', 'target/release')
    top = int(opt.get('--top', 12))
    os.makedirs(work, exist_ok=True)
    index = book_index(books)
    devices = ['kindle', 'ireader', 'xochitl']
    agree = {d: collections.Counter() for d in devices}
    total = collections.Counter()
    miss = {d: collections.Counter() for d in devices}
    ex = {}
    per_book = []
    key = lambda t: ''.join(t.split())[:60]
    for f in sorted(os.listdir(amz_dir)):
        if not f.endswith('.kfx'):
            continue
        amz = os.path.join(amz_dir, f)
        title = s2kcmp.title_of(amz) or f
        src = find_source(index, title, f)
        if not src:
            continue
        stem = os.path.join(work, re.sub(r'[/\\\\]', '_', title))
        for d in devices:
            subprocess.run([f'{bindir}/epub-optimize', f'--device={d}', src, f'{stem}.{d}.epub'], check=True, capture_output=True)
        subprocess.run([f'{bindir}/epub-to-kfx', f'{stem}.kindle.epub', f'{stem}.kindle.kfx'], check=True, capture_output=True)
        base_fs, _, _ = epub_nodes(f'{stem}.kindle.epub', bindir, 'kindle')
        sides = {'kindle': [(t, kfx_features(ch, base_fs)) for t, ch, _ in s2kcmp.nodes(f'{stem}.kindle.kfx')]}
        for d in ('ireader', 'xochitl'):
            bfs, blh, rows = epub_nodes(f'{stem}.{d}.epub', bindir, d)
            sides[d] = [(r[0], epub_features(r, bfs, blh)) for r in rows]
        queues = {d: collections.defaultdict(collections.deque) for d in devices}
        for d in devices:
            for t, feat in sides[d]:
                queues[d][key(t)].append(feat)
        n_book = 0
        book_agree = collections.Counter()
        for t, ch, _ in s2kcmp.nodes(amz):
            k = key(t)
            if not k or not all(queues[d][k] for d in devices):
                continue
            a = kfx_features(ch, base_fs)
            n_book += 1
            for ft in FEATURES:
                total[ft] += 1
            for d in devices:
                feat = queues[d][k].popleft()
                for ft in FEATURES:
                    if same(ft, a[ft], feat[ft]):
                        agree[d][ft] += 1
                        book_agree[d] += 1
                    else:
                        r = lambda v: round(v, 2) if isinstance(v, float) else v
                        kk = (ft, r(a[ft]), r(feat[ft]))
                        miss[d][kk] += 1
                        ex.setdefault((d, kk), f'{title}：{t[:24]}')
        if n_book:
            per_book.append((title, n_book, {d: book_agree[d] / (n_book * len(FEATURES)) for d in devices}))
    print('== 每本：比较的节点数，三台和 Send to Kindle 逐项一致的比例')
    for title, n, r in per_book:
        print(f'  {title}: {n} 个节点  ' + '  '.join(f'{d} {r[d]:.1%}' for d in devices))
    print('== 每项一致率（全部书）')
    print('  ' + '项目'.ljust(8) + ''.join(d.ljust(10) for d in devices))
    for ft in FEATURES:
        print('  ' + ft.ljust(8) + ''.join(f'{agree[d][ft] / max(total[ft], 1):.1%}'.ljust(10) for d in devices))
    for d in devices:
        print(f'== {d} 和 Send to Kindle 不一致最多的（项目, Amazon, {d}）: 节点数 «例子»')
        for kk, n in miss[d].most_common(top):
            print(f'  {n:7d}  {kk}  «{ex[(d, kk)]}»')


if __name__ == '__main__':
    main()
