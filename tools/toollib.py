"""tools/ 下各脚本共用的小工具：命令行参数、EPUB 的 OPF 定位、阅读模式 profile（regress/regresslib.py 和 kfx/ 的脚本都用）。"""
import functools
import html
import json
import os
import re
import subprocess
import sys


def cli_args(n, usage):
    """命令行位置参数（不含 `--` 开头的开关）；`-h`/`--help` 打印用法退出 0，少于 `n` 个打印用法退出 1。"""
    a = sys.argv[1:]
    if any(x in ('-h', '--help') for x in a):
        print(usage); sys.exit(0)
    pos = [x for x in a if not x.startswith('--')]
    if len(pos) < n:
        print(usage, file=sys.stderr); sys.exit(1)
    return pos


def cli_opts():
    """`--名字=值` 形式的开关 → {'--名字': 值}。"""
    return dict(a.split('=', 1) for a in sys.argv[1:] if a.startswith('--') and '=' in a)


def attr(tag, name):
    """标签里某个属性的值（单双引号都认，还原字符引用）；没有时 None。"""
    m = re.search(r'(?:^|\s)' + name + r'\s*=\s*(?:"([^"]*)"|\'([^\']*)\')', tag)
    return None if not m else html.unescape(m.group(1) if m.group(1) is not None else m.group(2))


def opf_path(z):
    """EPUB（打开的 ZipFile）里 OPF 的路径：container.xml 的 full-path（单双引号都认）。"""
    c = z.read('META-INF/container.xml').decode('utf-8', 'replace')
    return re.search(r'full-path\s*=\s*["\']([^"\']+)', c).group(1)


# 读 profile 用的 epub-optimize 所在目录：脚本的 --bin=（缺省 target/release，相对仓库根）。main 里拿到 --bin 后改它。
BINDIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'target', 'release')


@functools.lru_cache(maxsize=None)
def profile(device):
    """内置阅读模式解析好的 profile（`epub-optimize --print-profile`：缺省值都填上，键名同 TOML）。
    以前各脚本按行扫 TOML 自己填缺省（2026-10-15 改）：缺省值和 Rust 那边各写一份，`[section]` 里的同名键还会认错。"""
    out = subprocess.run([os.path.join(BINDIR, 'epub-optimize'), '--print-profile', f'--device={device}'], check=True, capture_output=True, text=True).stdout
    return json.loads(out)
