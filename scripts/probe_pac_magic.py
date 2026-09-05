# -*- coding: utf-8 -*-
"""P6.2 —— pac/ 逐包侦查:头校验 + 索引解析 + 条目魔数分布(抽样解压头 16B)。

- 普通包(magic YPF\\0):走 0x24 起的 XOR 0xC9 索引游走
- 头加密包(op.ypf/op_c.ypf 疑似):dump 头 64B 供定性
- 魔数判定:优先 zlib(78 01/9C/DA/5E)解压后取前 16B;否则原始头
"""
import collections
import os
import struct
import sys
import zlib

PAC = r"D:\yuris-kernel\AnimalTrailGirlishSquare 2\pac"
NAME_KEY = 0xC9

MAGIC_HINTS = [
    (b"\x89PNG", "PNG"),
    (b"BM", "BMP"),
    (b"GIF8", "GIF"),
    (b"\xff\xd8\xff", "JPEG"),
    (b"RIFF", "RIFF(WAV/AVI/WEBP)"),
    (b"OggS", "OGG"),
    (b"fLaC", "FLAC"),
    (b"PSB", "PSB"),
    (b"RIFF", "WEBP?"),
    (b"YSSD", "YSSD"),
    (b"YSTB", "YSTB"),
    (b"YSLB", "YSLB"),
    (b"YSVR", "YSVR"),
    (b"YSCF", "YSCF"),
    (b"YSCM", "YSCM"),
    (b"YSPG", "YSPG"),
    (b"YER", "YER?"),
]


def sniff(data):
    for magic, name in MAGIC_HINTS:
        if data.startswith(magic):
            return name
    return "raw:" + data[:4].hex()


def rcs(d, dp):
    out = bytearray()
    while d[dp] != 0:
        out.append(d[dp] ^ NAME_KEY)
        dp += 1
    return bytes(out), dp + 1


def parse_index(f, count, first_data):
    """游走索引区,返回条目列表 [(name, flag, uncomp, comp, off)]。"""
    f.seek(0x24)
    index_len = first_data - 0x24
    d = f.read(index_len)
    p = 0
    entries = []
    for _ in range(count):
        try:
            name, p = rcs(d, p)
        except IndexError:
            break
        flag = d[p]; p += 1
        uncomp, comp, off, _res = struct.unpack_from("<IIII", d, p); p += 16
        p += min(8, len(d) - p)
        entries.append((name.decode("cp932", "replace"), flag, uncomp, comp, off))
    return entries


def sample_magic(f, entries, n=400):
    """抽样读条目头(优先压缩流解压前 32B)。"""
    stats = collections.Counter()
    per_ext = collections.defaultdict(collections.Counter)
    if not entries:
        return stats, per_ext
    step = max(1, len(entries) // n)
    for (name, flag, uncomp, comp, off) in entries[::step][:n]:
        f.seek(off)
        raw = f.read(min(comp, 256))
        if flag == 1 and raw[:1] == b"\x78":
            try:
                head = zlib.decompress(raw * 1) if comp <= 256 else None
            except zlib.error:
                head = None
            if head is None:
                try:
                    d = zlib.decompressobj()
                    head = d.decompress(raw, 32)
                except zlib.error:
                    head = raw
        elif flag == 1:
            # 压缩但非 zlib 头:可能是加密;记原始头
            head = raw
        else:
            head = raw
        tag = sniff(head if isinstance(head, bytes) else raw)
        stats[tag] += 1
        ext = os.path.splitext(name)[1].lower() or "(none)"
        per_ext[ext][tag] += 1
    return stats, per_ext


def main():
    for fn in sorted(os.listdir(PAC)):
        if not fn.endswith(".ypf"):
            continue
        path = os.path.join(PAC, fn)
        with open(path, "rb") as f:
            head = f.read(0x24)
            magic = head[:4]
            if magic != b"YPF\x00":
                print(f"\n[{fn}] 头非 YPF\\0: {magic!r} —— dump 64B:")
                f.seek(0)
                d64 = f.read(64)
                print("  " + d64.hex(" "))
                print("  ascii: " + "".join(chr(c) if 32 <= c < 127 else "." for c in d64))
                continue
            version, count, first_data = struct.unpack_from("<III", head, 4)
            entries = parse_index(f, count, first_data)
            n_comp = sum(1 for e in entries if e[1] == 1)
            print(f"\n[{fn}] ver={version} count={count} first_data={first_data:#x}"
                  f" 解析={len(entries)} zlib={n_comp} stored={len(entries)-n_comp}")
            # 路径前缀分布(前 2 级)
            pref = collections.Counter()
            for (name, *_r) in entries:
                parts = name.replace("/", "\\").split("\\")
                pref["\\".join(parts[:2]) if len(parts) > 1 else "(root)"] += 1
            for k, v in pref.most_common(8):
                print(f"    {v:6d}  {k}")
            stats, per_ext = sample_magic(f, entries)
            print("  魔数抽样:", dict(stats.most_common(8)))
            for ext, cnt in sorted(per_ext.items()):
                print(f"    {ext:8s} {dict(cnt.most_common(4))}")


if __name__ == "__main__":
    main()
