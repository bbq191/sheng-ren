"""读 KFX 容器（分析用）：load(路径) → (容器信息, [(片段名, "$类型", 内容)])。

不再自己解 Ion：调 `kfx-dump --json`（crates/kfx 的 Rust 实现，产品代码同一套），JSON 的写法见 crates/kfx/src/bin/kfx_dump.rs 文件头。
kfx-dump 按这个顺序找：环境变量 KFX_DUMP、本仓库的 target/release/kfx-dump、PATH 里的 kfx-dump；
都没有时先 `cargo build --release -p kfx`。"""
import functools, json, os, shutil, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from toollib import cli_args, cli_opts  # noqa: E402,F401  各脚本从这里拿

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
