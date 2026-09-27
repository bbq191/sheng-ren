# 解析 FDST 与片段/骨架/目录索引：python3 indexes.py 文件.azw3
import sys, os; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import sys, struct; from kf8lib import *
f = F(sys.argv[1]); raw = rawml(f)
print('rawml len', len(raw), 'text length hdr', struct.unpack('>I', f.r0[4:8])[0])
fd = f.rec(f.u32(0xc0)); print('FDST', fd[:12].hex(), [struct.unpack('>II', fd[12+8*k:20+8*k]) for k in range(struct.unpack('>I', fd[8:12])[0])], 'len', len(fd))
for name, off in (('fragment', 0xf8), ('skeleton', 0xfc), ('ncx', 0xf4), ('guide', 0x104)):
    i = f.u32(off)
    if i == 0xffffffff: continue
    meta, ents = parse_indx(f, i)
    print('==', name, 'rec', i, {k: v for k, v in meta.items() if k != 'header_hex'})
    print('   header', meta['header_hex'])
    for e in ents[:6]: print('  ', e[0], e[1], e[2])
    if meta['ncncx']:
        c = f.rec(i + 1 + meta['nrec']); print('   CNCX first bytes', c[:80])
print('raw head:', raw[:700])
