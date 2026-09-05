# -*- coding: utf-8 -*-
"""P5.2 —— YSSD/snp(snappy raw)格式验证:解码全部 save/*.sd 块,断言载荷结构。"""
import struct
import glob
import os


def snappy_raw_decode(src):
    # varint 未压长度
    u = 0
    for i, b in enumerate(src):
        u |= (b & 0x7F) << (7 * i)
        if b < 0x80:
            break
    else:
        raise ValueError("varint too long")
    out = bytearray()
    p = i + 1
    while p < len(src):
        tag = src[p]
        t = tag & 3
        if t == 0:  # literal(变体:长度 = (tag>>2)+1,非标准 snappy 的 tag>>2)
            n = (tag >> 2) + 1
            p += 1
            if n > 0x3c:  # 60..63 → 1..4 额外长度字节
                extra = n - 1 - 59
                n = int.from_bytes(src[p:p + extra], "little") + 1
                p += extra
            out += src[p:p + n]
            p += n
        elif t == 1:  # copy1
            n = 4 + ((tag >> 2) & 7)
            off = ((tag >> 5) << 8) | src[p + 1]
            p += 2
            for _ in range(n):
                out.append(out[len(out) - off])
        elif t == 2:  # copy2
            n = (tag >> 2) + 1
            off = int.from_bytes(src[p + 1:p + 3], "little")
            p += 3
            for _ in range(n):
                out.append(out[len(out) - off])
        else:  # copy4
            n = (tag >> 2) + 1
            off = int.from_bytes(src[p + 1:p + 5], "little")
            p += 5
            for _ in range(n):
                out.append(out[len(out) - off])
    assert len(out) == u, "len mismatch %d != %d" % (len(out), u)
    return bytes(out)


def parse_payload(raw):
    t, dc = struct.unpack("<II", raw[:8])
    dims = struct.unpack("<%dI" % dc, raw[8:8 + dc * 4])
    dlen, = struct.unpack("<I", raw[8 + dc * 4:12 + dc * 4])
    body = raw[12 + dc * 4:]
    return t, dc, dims, dlen, body


TYPENAME = {1: "INT", 2: "FLT", 3: "STR"}

for f in sorted(glob.glob(r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\save\*.sd")):
    data = open(f, "rb").read()
    magic, ver, nblk, cap = struct.unpack("<4sIII", data[:16])
    tbl = struct.unpack("<1024I", data[0x10:0x1010])
    nz = [(i, v) for i, v in enumerate(tbl) if v]
    print("== %s: nblk=%d, %d non-zero table entries" % (os.path.basename(f), nblk, len(nz)))
    for i, v in nz:
        bid, btype, strict, varid, clen, ulen = struct.unpack("<IBBHII", data[v:v + 16])
        payload = data[v + 16:v + 16 + clen]
        try:
            raw = snappy_raw_decode(payload)
        except Exception as e:
            print("   block[%d] var@%d SNP FAIL: %s" % (i, varid, e))
            continue
        assert len(raw) == ulen, "ulen mismatch %d != %d" % (len(raw), ulen)
        t, dc, dims, dlen, body = parse_payload(raw)
        assert dlen == len(body), "dlen mismatch %d != %d" % (dlen, len(body))
        extra = ""
        if t == 3:
            # STR: sequence of {u32 len, bytes}
            q, cnt = 0, 0
            while q < len(body):
                sl, = struct.unpack("<I", body[q:q + 4])
                q += 4 + sl
                cnt += 1
            extra = " str_elems=%d" % cnt
        print("   block[%d] var@%d type=%s dims=%s bytes=%d%s" % (
            i, varid, TYPENAME.get(t, "?"), dims, dlen, extra))
