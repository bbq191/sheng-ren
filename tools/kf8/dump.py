# KF8/AZW3 结构查看（黑盒分析用）：python3 dump.py 文件.azw3
import sys, os; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from kf8lib import F, cli_args
f = F(cli_args(1, "用法：python3 dump.py 文件.azw3")[0]); d = f.d; n = f.n; r0 = f.r0
print('PDB name', d[:32].split(b'\0')[0], 'type/creator', d[60:68], 'records', n, 'seed', struct.unpack('>I', d[68:72])[0])
print('record attrs/ids first 4:', [d[78+8*i+4:78+8*i+8].hex() for i in range(min(4, n))])
print('palmdoc', struct.unpack('>HHIHHHH', r0[:16]))
hl = f.u32(20)
for off in range(0x14, 16 + hl, 4):
    print(f'  0x{off:02x}: {f.u32(off):#010x}')
e = 16 + hl; cnt = f.u32(e + 8); q = e + 12
print('EXTH count', cnt, 'len', f.u32(e + 4))
for _ in range(cnt):
    t, l = struct.unpack('>II', r0[q:q+8]); v = r0[q+8:q+l]
    print('  exth', t, v if len(v) != 4 else f'{struct.unpack(">I", v)[0]} ({v.hex()})'); q += l
fno, fnl = f.u32(0x54), f.u32(0x58); print('fullname', r0[fno:fno+fnl], 'r0 len', len(r0))
sigs = {}
for i in range(1, n):
    s = f.rec(i)[:4]
    sigs.setdefault(s if s.isalpha() or s in (b'\xe9\x8e\r\n',) else b'data', []).append(i)
for k, v in sigs.items(): print(k, len(v), v[:6], '...', v[-3:])
