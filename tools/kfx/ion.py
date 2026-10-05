"""Ion 1.0 二进制解码（只照公开规范 amzn.github.io/ion-docs/docs/binary.html 写），分析样本用。
没有名字的符号显示成 $N。产品代码用 crates/kfx 的 Rust 实现。"""
import struct, sys

class Sym(str):
    pass

class Annot:
    def __init__(self, anns, val): self.anns, self.val = anns, val
    def __repr__(self): return f"{'::'.join(self.anns)}::{self.val!r}"

SYS = ["$0", "$ion", "$ion_1_0", "$ion_symbol_table", "name", "version", "imports", "symbols",
       "max_id", "$ion_shared_symbol_table"]

def varuint(b, i):
    v = 0
    while True:
        c = b[i]; i += 1
        v = (v << 7) | (c & 0x7f)
        if c & 0x80: return v, i

def varint(b, i):
    c = b[i]; i += 1
    neg = c & 0x40; v = c & 0x3f
    if not c & 0x80:
        while True:
            c = b[i]; i += 1
            v = (v << 7) | (c & 0x7f)
            if c & 0x80: break
    return (-v if neg else v), i

def uint(b): return int.from_bytes(b, "big") if b else 0

def intf(b):
    if not b: return 0
    neg = b[0] & 0x80
    v = int.from_bytes(bytes([b[0] & 0x7f]) + b[1:], "big")
    return -v if neg else v

class Reader:
    def __init__(self, symtab=None):
        self.symbols = list(symtab or SYS)

    def sym(self, n):
        return Sym(self.symbols[n]) if n < len(self.symbols) else Sym(f"${n}")

    def values(self, b, i=0, end=None, top=True):
        end = len(b) if end is None else end
        out = []
        while i < end:
            if top and b[i:i+4] == b"\xe0\x01\x00\xea":
                i += 4; continue
            v, i = self.value(b, i)
            if v is _NOP: continue
            if top and isinstance(v, Annot) and v.anns and v.anns[0] == "$ion_symbol_table":
                self.load_symtab(v.val); continue
            out.append(v)
        return out

    def load_symtab(self, st):
        if not isinstance(st, dict): return
        if st.get("imports") == "$ion_symbol_table":
            pass
        else:
            self.symbols = list(SYS)
            for imp in st.get("imports") or []:
                n = imp.get("max_id", 0)
                base = len(self.symbols)
                self.symbols += [f"${base + k}" for k in range(n)]
                self.imported = imp
        self.symbols += [str(s) for s in st.get("symbols") or []]

    def value(self, b, i):
        td = b[i]; i += 1
        t, l = td >> 4, td & 0xf
        if t == 0 and l != 15:  # NOP pad
            if l == 14: l, i = varuint(b, i)
            return _NOP, i + l
        if l == 15: return None, i
        if t == 1: return bool(l), i
        if l == 14 and t not in (1,):
            l, i = varuint(b, i)
        if t == 0xD and l == 1:
            l, i = varuint(b, i)
        d = b[i:i+l]; j = i + l
        if t == 2: return uint(d), j
        if t == 3: return -uint(d), j
        if t == 4:
            if l == 0: return 0.0, j
            return struct.unpack(">f" if l == 4 else ">d", d)[0], j
        if t == 5:
            if l == 0: return ("dec", 0, 0), j
            e, k = varint(b, i)
            return ("dec", intf(b[k:j]), e), j
        if t == 6: return ("ts", d.hex()), j
        if t == 7: return self.sym(uint(d)), j
        if t == 8: return d.decode("utf-8", "replace"), j
        if t == 9: return ("clob", d), j
        if t == 0xA: return ("blob", d), j
        if t in (0xB, 0xC):
            return [v for v in self.values(b, i, j, top=False)], j
        if t == 0xD:
            out = {}; k = i
            while k < j:
                f, k = varuint(b, k)
                v, k = self.value(b, k)
                if v is _NOP: continue
                key = str(self.sym(f))
                if key in out:
                    n = 2
                    while f"{key}#{n}" in out: n += 1
                    key = f"{key}#{n}"
                out[key] = v
            return out, j
        if t == 0xE:
            al, k = varuint(b, i)
            anns = []; ae = k + al
            while k < ae:
                a, k = varuint(b, k); anns.append(str(self.sym(a)))
            v, _ = self.value(b, k)
            return Annot(anns, v), j
        raise ValueError(f"bad type {t:x} at {i-1}")

_NOP = object()

def short(v, depth=0, maxlen=200):
    if isinstance(v, tuple) and v and v[0] == "blob":
        return f"<blob {len(v[1])}B {v[1][:8].hex()}>"
    if isinstance(v, str) and len(v) > maxlen: return repr(v[:maxlen]) + "…"
    if isinstance(v, Annot): return f"{'::'.join(v.anns)}::{short(v.val, depth)}"
    if isinstance(v, dict):
        return "{" + ", ".join(f"{k}: {short(x, depth+1)}" for k, x in v.items()) + "}"
    if isinstance(v, list):
        if len(v) > 30: return "[" + ", ".join(short(x, depth+1) for x in v[:30]) + f", …(+{len(v)-30})]"
        return "[" + ", ".join(short(x, depth+1) for x in v) + "]"
    return repr(v)
