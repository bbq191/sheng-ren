"""tools/ 下各脚本共用的小工具：命令行参数、EPUB 的 OPF 定位（regress/regresslib.py 和 kfx/ 的脚本都用）。"""
import html
import re
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
