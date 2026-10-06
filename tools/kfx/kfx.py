"""读 KFX 容器：load(路径) → (符号表 Reader, 容器信息, [(片段名, 类型, 内容)])。分析用。"""
import struct, sys
from ion import *
def load(path):
    b=open(path,'rb').read()
    ver,hl,ci_off,ci_len=struct.unpack_from('<HIII',b,4)
    ci=Reader().values(b,ci_off,ci_off+ci_len)[0]
    r=Reader(); r.values(b,ci['$415'],ci['$415']+ci['$416'])
    ents=[]
    for k in range(ci['$414']//24):
        eid,t,o,l=struct.unpack_from('<IIQQ',b,ci['$413']+24*k)
        e=b[hl+o:hl+o+l]; h=struct.unpack_from('<HI',e,4)[1]
        rr=Reader(r.symbols); rr.values(e,10,h)
        try: body=rr.values(e,h)
        except Exception: body=('raw',e[h:])
        ents.append((str(r.sym(eid)),f'${t}',body))
    return r,ci,ents

def cli_args(n, usage):
    """命令行位置参数（不含 `--` 开头的开关）；`-h`/`--help` 打印用法退出 0，少于 `n` 个打印用法退出 1。"""
    import sys
    a = sys.argv[1:]
    if any(x in ('-h', '--help') for x in a):
        print(usage); sys.exit(0)
    pos = [x for x in a if not x.startswith('--')]
    if len(pos) < n:
        print(usage, file=sys.stderr); sys.exit(1)
    return pos
