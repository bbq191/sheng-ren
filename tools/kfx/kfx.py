"""读 KFX 容器（分析用）：load(路径) → (容器信息, [(片段名, "$类型", 内容)])。

不再自己解 Ion：调 `kfx-dump --json`（crates/kfx 的 Rust 实现，产品代码同一套），JSON 的写法见 crates/kfx/src/bin/kfx_dump.rs 文件头。
kfx-dump 按这个顺序找：环境变量 KFX_DUMP、本仓库的 target/release/kfx-dump、PATH 里的 kfx-dump；
都没有时先 `cargo build --release -p kfx`。"""
import functools, json, os, shutil, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from toollib import cli_args, cli_opts, opf_path  # noqa: E402,F401  各脚本从这里拿

_REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


def kfx_dump():
    for p in (os.environ.get('KFX_DUMP'), os.path.join(_REPO, 'target', 'release', 'kfx-dump'), shutil.which('kfx-dump')):
        if p and os.path.isfile(p) and os.access(p, os.X_OK):
            return p
    sys.exit('找不到 kfx-dump：先运行 cargo build --release -p kfx（或用环境变量 KFX_DUMP 指定）')


@functools.lru_cache(maxsize=4)
def load(path):
    """同一本书只解析一次（s2kbatch、s2kdev 先取书名再比，以前各解析一遍）；返回的结构别就地改。"""
    out = subprocess.run([kfx_dump(), '--json', path], capture_output=True, check=True).stdout
    d = json.loads(out)
    return d['info'], [tuple(e) for e in d['entities']]


@functools.lru_cache(maxsize=4)
def by_type(path):
    """{"$类型": {片段名: 内容}}；片段内容是单元素列表的取出那个元素（pair、s2kcmp、tree 共用）。返回的结构别就地改。"""
    E = {}
    for i, t, b in load(path)[1]:
        E.setdefault(t, {})[i] = b[0] if isinstance(b, list) and b else b
    return E


def pools(E):
    """文字池：片段名 → 字符串列表（文字节点的 `$145` 是 {name: 池名, $403: 序号}）。"""
    return {k: v['$146'] for k, v in E.get('$145', {}).items()}


def reading_order(E):
    """阅读顺序里的版面（section）名（第一个阅读顺序）。"""
    return E['$258'][list(E['$258'])[0]]['$169'][0]['$170']


def text_nodes(E):
    """按阅读顺序走遍各版面的 storyline，产出 (文字, 节点链)：节点链是从版面顶层到这个文字节点的节点（含它自己）。
    只算引用了文字池的节点；storyline 找不到的版面模板跳过。"""
    pl = pools(E)

    def walk(n, chain):
        chain = chain + [n]
        t = n.get('$145')
        if isinstance(t, dict) and t.get('name') in pl:
            yield pl[t['name']][t['$403']], chain
        for k in n.get('$146', []) or []:
            if isinstance(k, dict):
                yield from walk(k, chain)

    for s in reading_order(E):
        for pt in E['$260'][s]['$141']:
            sl = E['$259'].get(pt.get('$176'))
            for n in (sl or {}).get('$146', []):
                yield from walk(n, [])


def short(v, depth=0, maxlen=200):
    """值的简短写法（长字符串、长列表截断；blob/资源写字节数）。"""
    if isinstance(v, dict) and ('$blob' in v or '$raw' in v or '$clob' in v):
        k = next(k for k in ('$blob', '$raw', '$clob') if k in v)
        return f"<{k[1:]} {v[k]}B {v.get('$head', '')}>"
    if isinstance(v, dict) and '$annot' in v:
        return f"{'::'.join(v['$annot'])}::{short(v['$value'], depth)}"
    if isinstance(v, str) and len(v) > maxlen:
        return repr(v[:maxlen]) + "…"
    if isinstance(v, dict):
        return "{" + ", ".join(f"{k}: {short(x, depth + 1)}" for k, x in v.items()) + "}"
    if isinstance(v, list):
        if len(v) > 30:
            return "[" + ", ".join(short(x, depth + 1) for x in v[:30]) + f", …(+{len(v) - 30})]"
        return "[" + ", ".join(short(x, depth + 1) for x in v) + "]"
    return repr(v)
