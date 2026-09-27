# KF8/AZW3 结构查看（黑盒分析用）：python3 dump.py 文件.azw3
import struct, sys
d = open(sys.argv[1], 'rb').read()
n = struct.unpack('>H', d[76:78])[0]
offs = [struct.unpack('>I', d[78+8*i:82+8*i])[0] for i in range(n)] + [len(d)]
rec = lambda i: d[offs[i]:offs[i+1]]
print('PDB name', d[:32].split(b'\0')[0], 'type/creator', d[60:68], 'records', n, 'seed', struct.unpack('>I', d[68:72])[0])
print('record attrs/ids first 4:', [d[78+8*i+4:78+8*i+8].hex() for i in range(4)])
r0 = rec(0)
print('palmdoc', struct.unpack('>HHIHHHH', r0[:16]))
hl = struct.unpack('>I', r0[20:24])[0]
for off in range(0x14, 16 + hl, 4):
    print(f'  0x{off:02x}: {struct.unpack(">I", r0[off:off+4])[0]:#010x}')
e = 16 + hl; cnt = struct.unpack('>I', r0[e+8:e+12])[0]; q = e + 12
print('EXTH count', cnt, 'len', struct.unpack('>I', r0[e+4:e+8])[0])
for _ in range(cnt):
    t, l = struct.unpack('>II', r0[q:q+8]); v = r0[q+8:q+l]
    print('  exth', t, v if len(v) != 4 else f'{struct.unpack(">I", v)[0]} ({v.hex()})'); q += l
fno, fnl = struct.unpack('>II', r0[0x54:0x5c]); print('fullname', r0[fno:fno+fnl], 'r0 len', len(r0))
sigs = {}
for i in range(1, n):
    s = rec(i)[:4]
    sigs.setdefault(s if s.isalpha() or s in (b'\xe9\x8e\r\n',) else b'data', []).append(i)
for k, v in sigs.items(): print(k, len(v), v[:6], '...', v[-3:])
