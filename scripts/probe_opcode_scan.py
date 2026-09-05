#!/usr/bin/env python3
"""P1.2 —— 全量 YSTB content 区变长字节码扫描。

用法:
    python3 scripts/probe_opcode_scan.py <bn.ypf路径> [--max-scripts N]

回答的问题(全部逐条断言, 不接受总量近似):
  Q1 密钥 2b904f93 是否对全部脚本成立(逐脚本 contiguity == 1.0)
  Q2 自描述编码 [op:u8][operand_len:u16 LE][operand] 是否在每一个槽位精确闭合
  Q3 opcode 种类数是否收敛(收敛性检验, 见 docs/03-phase1-plan.md P1.2)
  Q4 全语料 tag 直方图 + 各 tag 的 M-string(0x4D) 载荷样本

编码假说(待本脚本验证):
    instruction = op(1B) + operand_len(2B LE) + operand(operand_len B)
    注: [op][len:u8][flag:u8] 与 [op][len:u16LE] 在 flag==0 时字节等价,
        本样本若从未出现第三字节非 0, 则两种读法不可区分 —— 如实记录。
"""
import struct
import sys
import zlib
from collections import Counter, defaultdict

NAME_XOR_KEY = 0xC9
SAMPLE_KEY = bytes.fromhex("2b904f93")


def read_cstring_xor(data, p, key):
    out = bytearray()
    while data[p] != 0:
        out.append(data[p] ^ key)
        p += 1
    return out.decode(), p + 1


def parse_ypf(d):
    magic, ver, cnt, data0 = struct.unpack_from("<4sIII", d, 0)
    assert magic == b"YPF\0"
    p = 0x24
    entries = []
    for _ in range(cnt):
        name, p = read_cstring_xor(d, p, NAME_XOR_KEY)
        flags = d[p]; p += 1
        uncomp, comp, off, _res = struct.unpack_from("<IIII", d, p); p += 16
        tail = min(8, data0 - p)          # 末条 tail 可能不足 8 字节(见 ypf.md §2.4)
        p += tail
        entries.append((name, flags, uncomp, comp, off))
    assert abs(p - data0) <= 8, (hex(p), hex(data0))
    return ver, entries


def read_entry(d, e):
    name, flags, uncomp, comp, off = e
    raw = d[off:off + comp]
    return zlib.decompress(raw) if flags == 1 else raw


def xor_body(y, key):
    body = bytearray(y)
    for i in range(0x20, len(body)):
        body[i] ^= key[i % 4]
    return bytes(body)


def parse_ystb(y, key):
    magic, ver, unk1, p1, cl, sl, p4, unk2 = struct.unpack_from("<4s7I", y, 0)
    assert magic == b"YSTB", magic
    assert 0x20 + p1 + cl + sl + p4 == len(y), "YSTB 分区不闭合"
    assert cl % 12 == 0
    body = xor_body(y, key)
    cmds = body[0x20 + p1: 0x20 + p1 + cl]
    content = body[0x20 + p1 + cl: 0x20 + p1 + cl + sl]
    slots = []
    for i in range(cl // 12):
        tag, ln, off = struct.unpack_from("<III", cmds, i * 12)
        slots.append((tag, ln, off))
    return ver, slots, content


def guess_key_file(y):
    """逐文件密钥猜测。关键点: 命令区从 body 偏移 p1 开始,
    cipher[s] = plain[s] ^ key[(p1 + s) % 4], 候选必须按相位散布。
    K1: 首槽位 offset 恒为 0  -> key[(p1+8+t)%4] = c[8+t]
    K2: 首槽位 tag 常为 0     -> key[(p1+t)%4]   = c[t]
    取连续性高者。"""
    magic, ver, unk1, p1, cl, sl, p4, unk2 = struct.unpack_from("<4s7I", y, 0)
    c = y[0x20 + p1: 0x20 + p1 + cl]
    phase = p1 % 4
    cands = []
    if len(c) >= 12:
        k1 = [0] * 4
        k2 = [0] * 4
        for t in range(4):
            k1[(phase + 8 + t) % 4] = c[8 + t]
            k2[(phase + t) % 4] = c[t]
        cands = [bytes(k1), bytes(k2)]
    best = None
    for k in cands:
        try:
            _, slots, content = parse_ystb(y, k)
        except AssertionError:
            continue
        s = contiguity(slots, len(content))
        if best is None or s > best[1]:
            best = (k, s)
    return best


def contiguity(slots, content_len):
    ok, prev = 0, 0
    for tag, ln, off in slots:
        if off == prev and off + ln <= content_len:
            ok += 1
        prev = off + ln
    return ok / len(slots) if slots else 1.0


def segment(buf):
    """按 [op][len:u16 LE][operand] 切分, 返回 [(op, len, operand_bytes)]。
    不闭合则抛 ValueError(带位置)。"""
    out = []
    p = 0
    n = len(buf)
    while p < n:
        if p + 3 > n:
            raise ValueError(f"truncated header at {p}: {buf[p:].hex()}")
        op = buf[p]
        ln = buf[p + 1] | (buf[p + 2] << 8)
        end = p + 3 + ln
        if end > n:
            raise ValueError(f"operand overrun at {p}: op={op:02x} len={ln} end={end}>{n}")
        out.append((op, ln, buf[p + 3:end]))
        p = end
    return out


def main():
    path = sys.argv[1]
    max_scripts = None
    if "--max-scripts" in sys.argv:
        max_scripts = int(sys.argv[sys.argv.index("--max-scripts") + 1])
    d = open(path, "rb").read()
    ver, entries = parse_ypf(d)
    print(f"[YPF] version={ver} entries={len(entries)}")

    scripts = [e for e in entries if e[0].startswith("$ysbin\\yst")
               and e[0].endswith(".ybn") and "list" not in e[0]]
    print(f"[SCAN] scripts found = {len(scripts)}")

    op_count = Counter()              # op -> 次数
    op_lens = defaultdict(Counter)    # op -> operand_len 分布
    op_flags = Counter()              # header 第 3 字节非 0 计数(判 u8+u8+u8 vs u16)
    tag_count = Counter()             # tag -> 次数
    tag_m_payloads = defaultdict(Counter)  # tag -> M-string payload -> 次数
    tag_op0 = defaultdict(Counter)    # tag -> 首 opcode 分布
    key_hist = Counter()             # 逐文件猜出的密钥 -> 文件数
    key_bad, seg_bad = [], []
    total_slots = 0
    total_instr = 0
    scanned = 0

    for e in scripts:
        if max_scripts and scanned >= max_scripts:
            break
        scanned += 1
        name = e[0]
        y = read_entry(d, e)
        g = guess_key_file(y)
        if g is None or g[1] != 1.0:
            key_bad.append((name, f"best={g[1]:.4f}" if g else "no candidate"))
            continue
        key, _ = g
        key_hist[key.hex()] += 1
        v, slots, content = parse_ystb(y, key)
        total_slots += len(slots)
        for tag, ln, off in slots:
            tag_count[tag] += 1
            buf = content[off:off + ln]
            try:
                instrs = segment(buf)
            except ValueError as ex:
                seg_bad.append((name, tag, ex))
                continue
            if instrs:
                tag_op0[tag][instrs[0][0]] += 1
            for op, olen, payload in instrs:
                op_count[op] += 1
                op_lens[op][olen] += 1
                if op == 0x4D:
                    tag_m_payloads[tag][payload] += 1
            total_instr += len(instrs)

    print(f"\n[SCAN] scanned={scanned} scripts, slots={total_slots}, instructions={total_instr}")
    print(f"[SCAN] key failures: {len(key_bad)}{key_bad[:5] if key_bad else ''}")
    print(f"[SCAN] segmentation failures: {len(seg_bad)}")
    for f in seg_bad[:10]:
        print(f"    {f}")

    print(f"\n[Q1] per-file key guess: "
          f"{'PASS (all contiguity==1.0)' if not key_bad else f'FAIL x{len(key_bad)}'}")
    print(f"[Q1] recovered key histogram (per FILE): {dict(key_hist)}")
    print(f"[Q2] per-slot exact closure: "
          f"{'PASS (0 failures)' if not seg_bad else f'FAIL x{len(seg_bad)}'}")

    print(f"\n[Q3] distinct opcodes = {len(op_count)}  (收敛性: 几十~几百=假说成立, 上万=假说错误)")
    print(f"[Q3] flag-byte nonzero count = {sum(v for k, v in op_flags.items())}")
    # operand_len 是否每 op 唯一
    multi = {op: dict(c) for op, c in op_lens.items() if len(c) > 1}
    print(f"[Q3] ops with MULTIPLE operand lengths: {len(multi)}")
    for op, c in sorted(multi.items())[:20]:
        print(f"    op 0x{op:02x}: {dict(sorted(c.items()))}")

    print("\n[Q3] opcode frequency (top 40):")
    for op, n in op_count.most_common(40):
        lens = dict(sorted(op_lens[op].items()))
        print(f"    0x{op:02x}  x{n:<7} operand_len={lens}")
    print(f"    ... 共 {len(op_count)} 种; 尾部: "
          f"{[f'0x{op:02x}x{n}' for op, n in op_count.most_common()[-8:]]}")

    print("\n[Q4] corpus tag histogram:")
    for tag, n in sorted(tag_count.items()):
        print(f"    0x{tag:08X}  x{n}")
        tops = tag_op0[tag].most_common(3)
        print(f"        first-opcode: {[(f'0x{o:02x}', c) for o, c in tops]}")
        if tag_m_payloads[tag]:
            samples = tag_m_payloads[tag].most_common(10)
            print(f"        M-payloads: {[(p[:48], c) for p, c in samples]}")

    # 最大 operand_len 与 M 载荷长度上限
    max_len = max((olen for c in op_lens.values() for olen in c), default=0)
    print(f"\n[INFO] max operand_len observed = {max_len}")


if __name__ == "__main__":
    main()
