"""一批 Send to Kindle 的 KFX 逐本对照我们的写出器，汇总全部书的样式差异。
用法：python3 s2kbatch.py <Amazon KFX 目录> <书目录> <工作目录> [--bin=target/release] [--opt=<epub-optimize>] [--device=kindle]
  --opt：发给 Send to Kindle 的 EPUB 是用哪一版优化器做的，就用那一版（输入不同时差异多半是输入造成的，不是转换规则）。
  每本 Amazon KFX 按元数据书名在书目录里找原书（OPF 的 dc:title，找不到再按文件名），用 epub-optimize 和 epub-to-kfx
  生成我们的 KFX 放进工作目录（书目录只读），再按文字配对比较（s2kcmp.compare）。
  输出：每本的配上率；全部书合起来的差异（属性, Amazon, 我们）：节点数、书数、例子。"""
import collections, os, re, subprocess, sys, zipfile
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kfx import cli_args, cli_opts, opf_path
import s2kcmp


def norm(s):
    s = re.sub(r'[\s　・·‧．.\-—_:：()（）《》「」\[\]]', '', s or '')
    return s.lower()


def opf_title(path):
    """OPF 里 dc:title 的文字；读不出来时 None。OPF 定位用 toollib.opf_path（单双引号都认）。"""
    try:
        with zipfile.ZipFile(path) as z:
            o = z.read(opf_path(z)).decode('utf-8', 'replace')
        m = re.search(r'<dc:title[^>]*>(.*?)</dc:title>', o, re.S)
        return re.sub(r'<[^>]+>', '', m.group(1)).strip() if m else None
    except Exception:
        return None


def book_index(books):
    """书目录里的文字书：规整后的书名（OPF 的 dc:title 和文件名各一个键）→ 路径。"""
    index = {}
    for root, _, files in os.walk(books):
        for f in files:
            if f.endswith('.epub') and '漫画' not in root:
                p = os.path.join(root, f)
                for k in {norm(opf_title(p)), norm(os.path.splitext(f)[0])}:
                    if k:
                        index.setdefault(k, p)
    return index


def find_source(index, title, kfx_name):
    """Amazon KFX 对应的原书：先按元数据书名，再按去掉 `_<32 位散列>` 的文件名。"""
    return index.get(norm(title)) or index.get(norm(re.sub(r'_[0-9A-F]{32}$', '', os.path.splitext(kfx_name)[0])))


def main():
    amz_dir, books, work = cli_args(3, __doc__)[:3]
    opt = cli_opts()
    bindir = opt.get('--bin', 'target/release')
    device = opt.get('--device', 'kindle')
    optimizer = opt.get('--opt', f'{bindir}/epub-optimize')
    os.makedirs(work, exist_ok=True)
    index = book_index(books)
    leaf, chain = collections.Counter(), collections.Counter()
    books_of, ex = collections.defaultdict(set), {}
    rows = []
    for f in sorted(os.listdir(amz_dir)):
        if not f.endswith('.kfx'):
            continue
        amz = os.path.join(amz_dir, f)
        title = s2kcmp.title_of(amz) or os.path.splitext(f)[0]
        src = find_source(index, title, f)
        if not src:
            rows.append((title, '没找到原书'))
            continue
        stem = os.path.join(work, re.sub(r'[/\\\\]', '_', title))
        try:
            subprocess.run([optimizer, f'--device={device}', src, stem + '.epub'], check=True, capture_output=True)
            subprocess.run([f'{bindir}/epub-to-kfx', stem + '.epub', stem + '.kfx'], check=True, capture_output=True)
            m, um, na, nb, lf, chn, runs, e = s2kcmp.compare(amz, stem + '.kfx')
        except Exception as err:
            rows.append((title, f'失败：{err}'))
            continue
        rows.append((title, f'配上 {m}/{na}（我们 {nb}）{"  ⚠ 配上率低，可能不是同一版" if na and m < 0.9 * na else ""}  {src}'))
        if na and m < 0.9 * na:
            continue
        for k, n in lf.items():
            leaf[k] += n
            books_of[k].add(title)
            ex.setdefault(k, (title, e[k]))
        for k, n in chn.items():
            chain[k] += n
            books_of[k].add(title)
            ex.setdefault(k, (title, e[k]))
    print('== 每本')
    for t, s in rows:
        print(f'  {t}: {s}')
    print('== 文字节点的样式差异（属性, Amazon, 我们）: 节点数 书数 «例子»')
    for k, n in sorted(leaf.items(), key=lambda kv: (-len(books_of[kv[0]]), -kv[1]))[:150]:
        print(f'{n:7d} {len(books_of[k]):3d}  {k}  «{ex[k][0]}：{ex[k][1]}»')
    print('== 容器链差异: 节点数 书数')
    for k, n in sorted(chain.items(), key=lambda kv: (-len(books_of[kv[0]]), -kv[1]))[:40]:
        print(f'{n:7d} {len(books_of[k]):3d}  Amazon {list(k[0])}\n             我们   {list(k[1])}  «{ex[k][0]}：{ex[k][1]}»')


if __name__ == '__main__':
    main()
