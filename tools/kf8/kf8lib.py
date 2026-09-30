# KF8/AZW3 黑盒分析小库：PDB 记录、PalmDOC 解压、尾随字节、INDX/TAGX 解析。只读文件，不改任何东西。
import struct
class F:
    def __init__(s, p):
        d = s.d = open(p, 'rb').read()
        n = struct.unpack('>H', d[76:78])[0]
        s.offs = [struct.unpack('>I', d[78+8*i:82+8*i])[0] for i in range(n)] + [len(d)]
        s.n = n; s.r0 = s.rec(0)
    def rec(s, i): return s.d[s.offs[i]:s.offs[i+1]]
    def u32(s, off): return struct.unpack('>I', s.r0[off:off+4])[0]
def palmdoc_decompress(b):
    out = bytearray(); i = 0
    while i < len(b):
        c = b[i]; i += 1
        if c == 0 or 9 <= c <= 0x7f: out.append(c)
        elif 1 <= c <= 8: out += b[i:i+c]; i += c
        elif c >= 0xc0: out += b' ' + bytes([c ^ 0x80])
        else:
            c = (c << 8) | b[i]; i += 1
            dist = (c & 0x3fff) >> 3; ln = (c & 7) + 3
            for _ in range(ln): out.append(out[-dist])
    return bytes(out)
def strip_trailing(r, flags):
    def backward_int(b):
        v = 0; shift = 0
        for k in range(1, 5):
            x = b[-k]; v |= (x & 0x7f) << shift; shift += 7
            if x & 0x80: break
        return v
    for bit in range(15, 0, -1):
        if flags & (1 << bit): r = r[:-backward_int(r)]
    if flags & 1: r = r[:-((r[-1] & 3) + 1)]
    return r
def rawml(f):
    flags = f.u32(0xf0); cnt = struct.unpack('>H', f.r0[8:10])[0]
    return b''.join(palmdoc_decompress(strip_trailing(f.rec(i), flags)) for i in range(1, cnt + 1))
def fwd_int(b, i):
    v = 0
    while True:
        x = b[i]; i += 1; v = (v << 7) | (x & 0x7f)
        if x & 0x80: return v, i
def parse_indx(f, first):
    h = f.rec(first); hl, typ = struct.unpack('>II', h[4:12])
    idxt, nrec, enc = struct.unpack('>III', h[20:32]); total = struct.unpack('>I', h[36:40])[0]
    tagx = h[hl:]; assert tagx[:4] == b'TAGX'; tl, cb = struct.unpack('>II', tagx[4:12])
    tags = [tuple(tagx[12+4*k:16+4*k]) for k in range((tl-12)//4)]
    ncncx = struct.unpack('>I', h[52:56])[0]
    entries = []
    for r in range(first + 1, first + 1 + nrec):
        b = f.rec(r); ih = struct.unpack('>I', b[20:24])[0]; cnt = struct.unpack('>I', b[24:28])[0]
        assert b[ih:ih+4] == b'IDXT'
        pos = [struct.unpack('>H', b[ih+4+2*k:ih+6+2*k])[0] for k in range(cnt)] + [ih]
        for k in range(cnt):
            e = b[pos[k]:pos[k+1]]; kl = e[0]; key = e[1:1+kl]; i = 1 + kl
            ctrl = e[i:i+cb]; i += cb; vals = {}
            parsed = []
            for (tag, nv, mask, end) in tags:
                if end: ctrl = ctrl[1:]; continue
                v = ctrl[0] & mask
                if v == 0: continue
                if v == mask and bin(mask).count('1') > 1:
                    n, i = fwd_int(e, i); parsed.append((tag, nv, None, n))
                else:
                    while not mask & 1: mask >>= 1; v >>= 1
                    parsed.append((tag, nv, v, None))
            for tag, nv, cntv, nbytes in parsed:
                vs = []
                if cntv is not None:
                    for _ in range(cntv * nv): x, i = fwd_int(e, i); vs.append(x)
                else:
                    end = i + nbytes
                    while i < end: x, i = fwd_int(e, i); vs.append(x)
                vals[tag] = vs
            entries.append((key, vals, e.hex() if len(e) < 40 else e[:40].hex()))
    return dict(hl=hl, typ=typ, nrec=nrec, enc=enc, total=total, cb=cb, tags=tags, ncncx=ncncx, header_hex=h[:hl].hex()), entries
