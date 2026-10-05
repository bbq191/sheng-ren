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
