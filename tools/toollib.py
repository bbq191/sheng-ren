"""tools/ 下各脚本共用的小工具。"""
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
