"""Send to Kindle 的 KFX 对我们写出器的 KFX：按文字配对文字节点，逐属性汇总两边不一样的地方（同一本书、同一版 EPUB）。
用法：python3 s2kcmp.py amazon.kfx ours.kfx [条数]
输出三段：文字节点自己的样式差异 (属性, Amazon, 我们): 个数 «例子»；外层容器链的差异；行内区间有无的差异。
2026-10-08 用它对出写出器 11 的规则（见 docs/kfx.md#send-to-kindle-的样式规则）。"""
import collections
from kfx import by_type, cli_args, load, short, text_nodes

NAMES = {'$11': 'font', '$12': 'style', '$13': 'weight', '$16': 'size', '$19': 'color', '$34': 'align', '$36': 'indent',
         '$42': 'line_h', '$45': 'nowrap', '$47': 'm_top', '$48': 'm_left', '$49': 'm_bottom', '$50': 'm_right',
         '$52': 'p_top', '$53': 'p_left', '$54': 'p_bottom', '$55': 'p_right', '$56': 'width', '$70': 'bg',
         '$569': 'word_break', '$761': 'hints'}
UNITS = {'$308': 'em', '$310': 'lh', '$314': '%', '$318': 'pt', '$505': 'rem'}
SKIP = {'$173', '$10'}  # 样式名、语言


def fmt(v):
    if isinstance(v, dict) and '$307' in v:
        return f"{round(float(v['$307']), 3)}{UNITS.get(str(v['$306']), str(v['$306']))}"
    if isinstance(v, int) and v > 0xFFFFFF:
        return '#%08x' % v
    return short(v, maxlen=40)


def nodes(path):
    """[(文字, 样式链, 行内区间数)]：样式链是从版面顶层到文字节点每一层的样式（没有样式的层是 {}），按阅读顺序。"""
    E = by_type(path)
    styles = E.get('$157', {})

    def style(n):
        if '$157' not in n:
            return {}
        return {NAMES.get(k, k): fmt(v) for k, v in styles.get(str(n['$157']), {}).items() if k not in SKIP}

    return [(t, [style(n) for n in chain], len(chain[-1].get('$142', []) or [])) for t, chain in text_nodes(E)]


def compare(a_path, b_path):
    """两边配对比较，返回 (配上, 没配上, Amazon 节点数, 我们节点数, 叶子差异 Counter, 容器链差异 Counter, 行内区间差异 Counter, 例子)。"""
    a, b = nodes(a_path), nodes(b_path)
    key = lambda t: ''.join(t.split())[:60]
    queues = collections.defaultdict(collections.deque)
    for n in b:
        queues[key(n[0])].append(n)
    leaf, chain, runs = collections.Counter(), collections.Counter(), collections.Counter()
    ex = {}
    matched = unmatched = 0
    for t, ch, nr in a:
        k = key(t)
        if not k:
            continue
        if not queues[k]:
            unmatched += 1
            continue
        matched += 1
        _, ch2, nr2 = queues[k].popleft()
        la, lb = ch[-1], ch2[-1]
        for p in sorted(set(la) | set(lb)):
            if la.get(p, '-') != lb.get(p, '-'):
                kk = (p, la.get(p, '-'), lb.get(p, '-'))
                leaf[kk] += 1
                ex.setdefault(kk, t[:30])
        sig = (tuple(tuple(sorted(d.items())) for d in ch[:-1] if d), tuple(tuple(sorted(d.items())) for d in ch2[:-1] if d))
        if sig[0] != sig[1]:
            chain[sig] += 1
            ex.setdefault(sig, t[:30])
        if (nr > 0) != (nr2 > 0):
            runs[(nr > 0, nr2 > 0)] += 1
    return matched, unmatched, len(a), len(b), leaf, chain, runs, ex


def title_of(path):
    """KFX 元数据里的书名。"""
    ci, ents = load(path)
    for i, t, b in ents:
        if t == '$490':
            for cat in (b[0] if isinstance(b, list) else b).get('$491', []):
                for kv in cat.get('$258', []):
                    if kv.get('$492') == 'title':
                        return str(kv.get('$307'))
    return None


if __name__ == '__main__':
    pos = cli_args(2, __doc__)
    a_path, b_path = pos[:2]
    top = int(pos[2]) if len(pos) > 2 else 60
    matched, unmatched, na, nb, leaf, chain, runs, ex = compare(a_path, b_path)
    print(f'配上 {matched}，没配上 {unmatched}；Amazon {na} 个文字节点，我们 {nb} 个')
    print('== 文字节点的样式 (属性, Amazon, 我们): 个数 «例子»')
    for kk, n in leaf.most_common(top):
        print(f'{n:7d}  {kk}  «{ex[kk]}»')
    print('== 外层容器链')
    for sig, n in chain.most_common(top // 3):
        print(f'{n:7d}  Amazon {list(sig[0])}\n         我们   {list(sig[1])}  «{ex[sig]}»')
    print('== 行内区间（Amazon 有, 我们有）:', dict(runs))
