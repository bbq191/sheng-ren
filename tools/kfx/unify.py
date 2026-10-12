"""三台统一的回归门槛：以 Kindle 写出器的 KFX 为准，逐个文字节点、逐项比掌阅、Move 的 EPUB 交给阅读器的样式。
用法：python3 unify.py <书目录> <工作目录> [--bin=target/release] [--min=99.5] [--baseline=tools/kfx/unify-baseline.tsv] [--update] [--top=12] [书名子串…]

和 s2kdev.py 同一套折算和比较（字号、行高相对正文；上下边距正文 em；缩进本元素 em；左右边距页宽 %；颜色、字体、粗细、
斜体、对齐），区别是参考换成我们自己的 Kindle KFX：写出器已经照 Send to Kindle 的样本对齐过（docs/kfx.md），所以不要
Amazon 样本，书目录里的每本文字书都能比。掌阅、Move 的 `kindle_rules` 以「产物按 CSS 算出的样式 = 写出器的样式」为目标，
这里量的就是这个；阅读器自己的怪癖不在其中。
每本书一行一致率，列出不一致最多的项。漫画（路径里带「漫画」）不比。
图片（2026-10-12）：Kindle KFX 的图片节点按原始宽高和掌阅、Move 的图配对，比出没出现、宽度写法（百分比或原尺寸）、对齐；
另按 profile 查阅读器怪癖修没修好：Kindle 正文图片不能带透明（画成黑色）、`css_rgba = false` 的产物里不能有 `rgba(`、
`background_no_repeat = false` 的产物里不能有不平铺的背景图——这三条一处就不过，不看基线。
门槛：基线文件里记着的书，掌阅或 Move 的文字、图片一致率比记下的低 0.1 个百分点以上算变差；没记着的书低于 --min 算不过。有一本变差或不过就
退出码 1。基线里低于 --min 的书是已知差异（docs/kfx.md#三台统一的回归门槛2026-10-12 的五类），不变差就不报。
有意改进或接受新的差异以后，加 --update 把这次的一致率写回基线文件（只改这次比了的书）。"""
import collections, os, re, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kfx import cli_args, cli_opts
import s2kcmp
import kfx
from s2kdev import FEATURES, epub_features, epub_nodes, kfx_features, same

DEVICES = ['ireader', 'xochitl']
PROFILES = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'crates', 'profile', 'profiles')
ALIGN = {"'$320'": 'center', "'$61'": 'right', "'$59'": 'left', "'$321'": 'left'}


def profile_flag(device, key, default=True):
    """内置 profile 里的布尔字段（`key = true/false`），没写取缺省。"""
    for line in open(os.path.join(PROFILES, f'{device}.toml'), encoding='utf-8'):
        k, _, v = line.partition('=')
        if k.strip() == key:
            return v.split('#')[0].strip() == 'true'
    return default


def profile_value(device, key):
    """内置 profile 里的字符串字段（`key = "值"`），没写返回 None。"""
    for line in open(os.path.join(PROFILES, f'{device}.toml'), encoding='utf-8'):
        k, _, v = line.partition('=')
        if k.strip() == key:
            return v.split('#')[0].strip().strip('"')
    return None


def kfx_images(path, base_fs):
    """Kindle KFX 里的图片：([(原始宽, 高, 宽度写法, 对齐, 行内)] 按阅读顺序, 正文图片里带透明的个数, 背景图个数)。
    宽度写法：样式里写了百分比宽度是 `%:数`，em 折成根 em `em:数`，没写是 `-`（按原尺寸，大过页宽缩到页宽）。
    行内图片（嵌在字里）的 em 相对所在段落的字号，节点链上最近的字号就是段落的，同样折成根 em。"""
    E = kfx.by_type(path)
    styles = E.get('$157', {})
    res = {v['$175']: v for v in E.get('$164', {}).values() if isinstance(v, dict) and '$175' in v}
    raw = {k: v for k, v in E.get('$417', {}).items() if isinstance(v, dict)}
    out, alpha = [], set()
    for r, chain, alone, inline in kfx.image_nodes(E):
        st = [styles.get(str(n.get('$157')), {}) for n in chain]
        w = st[-1].get('$56')
        unit = {'$314': '%', '$308': 'em'}.get(w.get('$306')) if isinstance(w, dict) else None
        # em 相对图片节点自己的字号，KFX 的字号写成正文字号的倍数（rem）：乘上节点字号和全书正文字号（根 em），和 EPUB 那边
        # 按根 em 折的比（《嘯風山莊》正文 1.167em：402.75pt 的图 Kindle 写 28.76em）
        size = next((d['$16']['$307'] for d in reversed(st) if isinstance(d.get('$16'), dict)), 1.0)
        width = f"%:{w['$307']}" if unit == '%' else f"em:{w['$307'] * size * base_fs}" if unit == 'em' else '-'
        # 整页单图（封面这类）由版面居中，不比对齐
        align = '-' if alone else next((ALIGN.get(f"'{d['$34']}'", 'left') for d in reversed(st) if '$34' in d), 'left')
        meta = res.get(r, {})
        out.append((meta.get('$422', 0), meta.get('$423', 0), width, align, inline))
        if raw.get(f'{r}-ad', {}).get('$alpha'):
            alpha.add(r)
    bgs = {v['$479'] for v in styles.values() if isinstance(v, dict) and '$479' in v}
    return out, len(alpha), len(bgs)


def epub_images(path, bindir, device):
    """掌阅、Move 的 EPUB 交给阅读器的图片：([(原始宽, 高, CSS 宽度, 对齐, 在不在链接里, 行内)], [(背景图, 不平铺)])
    （`epub-to-kfx --styles` 的 `#img`、`#bg` 行；行内图片的宽度是显示宽度，根 em）。"""
    out = subprocess.run([f'{bindir}/epub-to-kfx', '--styles', f'--device={device}', path], check=True, capture_output=True, text=True).stdout.splitlines()
    imgs, bgs = [], []
    for line in out:
        f = line.split('\t')
        if f[0] == '#img' and len(f) == 8:
            imgs.append((int(f[2]), int(f[3]), f[4], {'center': 'center', 'right': 'right', 'end': 'right'}.get(f[5], 'left'), f[6] == 'true', f[7] == 'inline'))
        elif f[0] == '#bg' and len(f) == 3:
            bgs.append((f[1], f[2] == 'true'))
    return imgs, bgs


def same_width(a, b, w):
    """两种宽度写法显示出来一样宽：都按原尺寸、百分比相差不到 2，或者一边 100% 一边原图本来就宽过页面（都撑满页宽）。"""
    if a == b:
        return True
    # em 和 pt 都折成 em（12pt＝1em，不管元素自己的字号——图片多在正文里）比，相差 5% 以内算一样
    em = lambda x: float(x[3:]) if x.startswith('em:') else float(x[3:]) / 12 if x.startswith('pt:') else None
    ea, eb = em(a), em(b)
    if ea is not None and eb is not None:
        return abs(ea - eb) <= 0.05 * max(ea, eb)
    pct = lambda x: float(x.split(':')[1]) if x.startswith('%:') else None
    pa, pb = pct(a), pct(b)
    if pa is not None and pb is not None:
        return abs(pa - pb) < 2
    full = lambda p: p is not None and p >= 99
    return w >= 1000 and (full(pa) or full(pb)) and (pa is None or pb is None)


def epub_rgba_count(path):
    """EPUB 的样式表、XHTML 里还剩几处 `rgba(`（不认 `rgba()` 的阅读器上那条声明整条作废）。"""
    import zipfile
    z = zipfile.ZipFile(path)
    return sum(z.read(n).decode('latin-1').lower().count('rgba(') for n in z.namelist() if n.lower().endswith(('.css', '.xhtml', '.html', '.htm')))


def books_in(root, wanted):
    """书目录下的文字书（递归，跳过漫画）；给了书名子串的只要文件名含其中之一的。"""
    out = []
    for d, _, files in os.walk(root):
        if '漫画' in d:
            continue
        for f in files:
            if f.lower().endswith('.epub') and (not wanted or any(w in f for w in wanted)):
                out.append(os.path.join(d, f))
    return sorted(out)


def main():
    args = cli_args(2, __doc__)
    books, work, wanted = args[0], args[1], args[2:]
    opt = cli_opts()
    bindir = opt.get('--bin', 'target/release')
    floor = float(opt.get('--min', 99.5))
    base_path = opt.get('--baseline', os.path.join(os.path.dirname(os.path.abspath(__file__)), 'unify-baseline.tsv'))
    baseline = {}
    if os.path.exists(base_path):
        for line in open(base_path, encoding='utf-8'):
            if line.strip() and not line.startswith('#'):
                t, *v = line.rstrip('\n').split('\t')
                baseline[t] = dict(zip(['ireader', 'xochitl', 'img:ireader', 'img:xochitl'], map(float, v)))  # 后两列（图片）2026-10-12 起有
    seen = {}
    top = int(opt.get('--top', 12))
    os.makedirs(work, exist_ok=True)
    key = lambda t: ''.join(t.split())[:60]
    miss = {d: collections.Counter() for d in DEVICES}
    img_miss = {d: collections.Counter() for d in DEVICES}
    quirks = []
    ex = {}
    low = []
    print('== 每本：比较的节点数，掌阅、Move 和 Kindle 写出器逐项一致的比例')
    for src in books_in(books, wanted):
        title = os.path.splitext(os.path.basename(src))[0]
        stem = os.path.join(work, re.sub(r'[/\\\\]', '_', title))
        for d in ['kindle'] + DEVICES:
            subprocess.run([f'{bindir}/epub-optimize', f'--device={d}', src, f'{stem}.{d}.epub'], check=True, capture_output=True)
        subprocess.run([f'{bindir}/epub-to-kfx', f'{stem}.kindle.epub', f'{stem}.kindle.kfx'], check=True, capture_output=True)
        base_fs, _, _ = epub_nodes(f'{stem}.kindle.epub', bindir, 'kindle')
        queues = {}
        for d in DEVICES:
            bfs, blh, rows = epub_nodes(f'{stem}.{d}.epub', bindir, d)
            q = collections.defaultdict(collections.deque)
            for r in rows:
                q[key(r[0])].append(epub_features(r, bfs, blh, base_fs))
            queues[d] = q
        n = 0
        agree = collections.Counter()
        for t, ch, _ in s2kcmp.nodes(f'{stem}.kindle.kfx'):
            k = key(t)
            if not k or not all(queues[d][k] for d in DEVICES):
                continue
            ref = kfx_features(ch, base_fs)
            n += 1
            for d in DEVICES:
                feat = queues[d][k].popleft()
                for ft in FEATURES:
                    if same(ft, ref[ft], feat[ft]):
                        agree[d] += 1
                    else:
                        r = lambda v: round(v, 2) if isinstance(v, float) else v
                        kk = (ft, r(ref[ft]), r(feat[ft]))
                        miss[d][kk] += 1
                        ex.setdefault((d, kk), f'{title}：{t[:24]}')
        if not n:
            continue
        # 图片：Kindle 的图按原始宽高配对，比出没出现、宽度、对齐；再按 profile 查各台阅读器的怪癖修没修好
        k_imgs, k_alpha, k_bgs = kfx_images(f'{stem}.kindle.kfx', base_fs)
        img_agree = {}
        # 每张 Kindle 图在不在链接里：掌阅的 EPUB 图片一张不少，按原始宽高配上取它的标记（KFX 的图片节点不带链接）
        ref_imgs, _ = epub_images(f'{stem}.{DEVICES[0]}.epub', bindir, DEVICES[0])
        rq = collections.defaultdict(collections.deque)
        for im in ref_imgs:
            rq[(im[0], im[1])].append(im[4])
        k_linked = [rq[(w, h)].popleft() if rq[(w, h)] else False for w, h, *_ in k_imgs]
        if k_alpha:
            quirks.append(f'{title}：Kindle 正文图片里还有 {k_alpha} 张带透明的 PNG（Kindle 画成黑色）')
        for d in DEVICES:
            imgs, bgs = epub_images(f'{stem}.{d}.epub', bindir, d)
            icons_to_numbers = profile_value(d, 'note_icons') == 'number' 
            q = collections.defaultdict(collections.deque)
            for im in imgs:
                q[(im[0], im[1])].append(im)
            ok = total = 0
            for (w, h, width, align, inline), linked in zip(k_imgs, k_linked):
                total += 1
                if not q[(w, h)] and icons_to_numbers and linked:
                    ok += 1  # 只有图标的注释号换成了数字（profile note_icons = "number"，Move 里纯图链接点不动），设计如此
                    continue
                if not q[(w, h)]:
                    img_miss[d][('不见了', f'{w}×{h}')] += 1
                    ex.setdefault((d, ('不见了', f'{w}×{h}')), title)
                    continue
                _, _, ew, ea, _, ei = q[(w, h)].popleft()
                # 行内图片在段落里排，对齐跟着段落，不比
                checks = (('宽度', same_width(width, ew, w)), ('对齐', inline or align in ('-', ea)), ('行内/成块', inline == ei))
                bad = [k for k, good in checks if not good]
                if bad:
                    how = lambda x: '行内' if x else '成块'
                    kk = ('、'.join(bad), f'Kindle {width}/{align}/{how(inline)} {d} {ew}/{ea}/{how(ei)}')
                    img_miss[d][kk] += 1
                    ex.setdefault((d, kk), title)
                else:
                    ok += 1
            img_agree[d] = ok / total * 100 if total else 100.0
            if not profile_flag(d, 'css_rgba') and (c := epub_rgba_count(f'{stem}.{d}.epub')):
                quirks.append(f'{title}：{d} 不认 rgba()，产物里还有 {c} 处')
            if not profile_flag(d, 'background_no_repeat') and (c := sum(1 for _, nr in bgs if nr)):
                quirks.append(f'{title}：{d} 不认背景图 no-repeat，产物里还有 {c} 个不平铺的背景图')
        rate = {d: agree[d] / (n * len(FEATURES)) * 100 for d in DEVICES}
        rate.update({f'img:{d}': img_agree[d] for d in DEVICES})
        seen[title] = rate
        known = baseline.get(title)
        cols = [k for k in rate if known and k in known] if known else []
        if known:
            worse = any(rate[k] < known[k] - 0.1 for k in cols) or any(rate[f'img:{d}'] < floor for d in DEVICES if f'img:{d}' not in known)
            flag = f'  ✗ 比基线差（{"/".join(f"{known[k]:.1f}" for k in cols)}）' if worse else ('  （已知差异）' if min(known.values()) < floor else '')
        else:
            worse = min(rate.values()) < floor
            flag = '  ✗ 低于门槛' if worse else ''
        imgs = f'  图片 {len(k_imgs)} 张：' + ' '.join(f'{d} {img_agree[d]:.0f}%' for d in DEVICES) if k_imgs else ''
        print(f'  {title}: {n} 个节点  ' + '  '.join(f'{d} {rate[d]:.1f}%' for d in DEVICES) + imgs + flag)
        if worse:
            low.append(title)
    for d in DEVICES:
        print(f'== {d} 和 Kindle 不一致最多的（项目, Kindle, {d}）: 节点数 «例子»')
        for kk, c in miss[d].most_common(top):
            print(f'  {c:7d}  {kk}  «{ex[(d, kk)]}»')
    for d in DEVICES:
        if img_miss[d]:
            print(f'== {d} 的图片和 Kindle 不一致（项目, 两边的宽度/对齐）: 张数 «例子»')
            for kk, c in img_miss[d].most_common(top):
                print(f'  {c:7d}  {kk}  «{ex[(d, kk)]}»')
    if quirks:
        print('✗ 阅读器怪癖没处理好（不看基线，一处就不过）：')
        for q in quirks:
            print('  ' + q)
    if '--update' in sys.argv:
        baseline.update(seen)
        with open(base_path, 'w', encoding='utf-8') as f:
            f.write('# tools/kfx/unify.py 的基线：书名\t掌阅文字一致率\tMove 文字一致率\t掌阅图片一致率\tMove 图片一致率（%）。--update 写回；低于门槛的是已知差异，见 docs/kfx.md\n')
            for t in sorted(baseline):
                f.write(t + '\t' + '\t'.join(f'{baseline[t][k]:.2f}' for k in ['ireader', 'xochitl', 'img:ireader', 'img:xochitl'] if k in baseline[t]) + '\n')
        print(f'已写回基线 {base_path}（{len(seen)} 本）')
        return
    if low or quirks:
        if low:
            print('✗ 变差或不过门槛的书：' + '、'.join(low))
        sys.exit(1)
    print(f'✓ 没有书变差，新书都不低于 {floor}%')


if __name__ == '__main__':
    main()
