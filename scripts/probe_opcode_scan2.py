#!/usr/bin/env python3
"""P1.2 v2 —— 全语料 YSTB 扫描(通用逐文件猜钥 + 窗口模型检验)。

用法:
    python3 scripts/probe_opcode_scan2.py <bn.ypf路径>

v1 (probe_opcode_scan.py) 的结论与问题:
  - 自描述编码 [op:u8][operand_len:u16 LE][operand] 在 74 个"顺序型"脚本上
    26,415 槽位零失败 -> Confirmed
  - 但密钥并非常量 2b904f93; 且内容池存在重叠窗口(tag0 文本窗口是其他窗口的前缀)

v2 改动:
  1. 通用猜钥: key[(p1+1)%4] / [(p1+2)%4] / [(p1+3)%4] 由"恒零字段"多数投票
     (tag byte1=index高字节, len byte2=len>>16, tag byte3), 第 4 字节暴力 256,
     评分 = 槽位合理性 (tag<0x40000 且 off+len<=content_len 的比例)。
     不依赖 slot0 的任何假设。
  2. 逐槽位分段闭合断言只施加于 **非 tag0** 槽位(tag0 视为窗口注记, 单独统计)。
  3. tag0 窗口做"前缀检验": 其字节范围是否被某个非 tag0 窗口包含。
  4. 输出 opcode 频次、每 op 操作数宽度、每 tag 首opcode、M-payload 样本。
"""
import struct
import sys
import zlib
from collections import Counter, defaultdict

NAME_XOR_KEY = 0xC9


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
        p += min(8, data0 - p)
        entries.append((name, flags, uncomp, comp, off))
    return ver, entries


def read_entry(d, e):
    name, flags, uncomp, comp, off = e
    raw = d[off:off + comp]
    return zlib.decompress(raw) if flags == 1 else raw


def ystb_regions(y):
    magic, ver, unk1, p1, cl, sl, p4, unk2 = struct.unpack_from("<4s7I", y, 0)
    assert magic == b"YSTB", magic
    assert 0x20 + p1 + cl + sl + p4 == len(y), "YSTB 分区不闭合"
    return ver, unk1, p1, cl, sl, p4


def decrypt_body(y, key):
    b = bytearray(y)
    for i in range(0x20, len(b)):
        b[i] ^= key[i % 4]
    return bytes(b)


def slots_of(body, p1, cl):
    cmds = body[0x20 + p1: 0x20 + p1 + cl]
    n = cl // 12
    return [struct.unpack_from("<III", cmds, i * 12) for i in range(n)]


def recover_key(y):
    """通用密钥恢复 v3。
    相位 1/2/3 由恒零字段多数投票(tag byte1 / len byte2 / tag byte3),
    自由相位先用 tag 首字节 <= 0x78 预筛, 再用「非 tag0 窗口分段精确闭合」裁决。
    返回 (key, seg_fail_count, slots, content) 或 None。"""
    ver, unk1, p1, cl, sl, p4 = ystb_regions(y)
    if cl < 12:
        return None
    c = y[0x20 + p1: 0x20 + p1 + cl]
    n = cl // 12
    ph = p1 % 4
    key = [None] * 4
    for field_off, key_phase in ((1, (ph + 1) % 4), (6, (ph + 2) % 4), (3, (ph + 3) % 4)):
        votes = Counter()
        for i in range(n):
            votes[c[12 * i + field_off]] += 1
        key[key_phase] = votes.most_common(1)[0][0]
    free_phase = (ph + 0) % 4

    # 预筛: tag 首字节(= index 低字节)应 <= 0x78; 再按 index==0 槽位数降序
    # (index 0x00 在语料中占绝对多数), 使正确候选最先被尝试。
    scored = []
    for cand in range(256):
        zero_cnt = sum(1 for i in range(n) if (c[12 * i] ^ cand) == 0)
        if zero_cnt == 0:
            continue
        scored.append((zero_cnt, cand))
    scored.sort(reverse=True)

    best = None
    for _score0, cand in scored:
        k = list(key)
        k[free_phase] = cand
        kb = bytes(k)
        body = decrypt_body(y, kb)
        content = body[0x20 + p1 + cl: 0x20 + p1 + cl + sl]
        slots = slots_of(body, p1, cl)
        fails = 0
        testable = 0
        for tag, ln, off in slots:
            if tag == 0:
                continue                     # tag0 = 窗口注记, 不要求闭合
            if ln == 0 or off + ln > len(content):
                continue
            testable += 1
            try:
                segment(content[off:off + ln])
            except ValueError:
                fails += 1
                if best is not None and (fails, -testable) > (best[1], -best[2]):
                    break
        # 必须有足够多可测窗口, 否则错密钥会靠"全部越界被跳过"作弊
        if testable < 5:
            continue
        cand_res = (kb, fails, testable, slots, content)
        if best is None or (fails, -testable) < (best[1], -best[2]):
            best = cand_res
        if fails == 0:
            break
    return best


def segment(buf):
    out = []
    p = 0
    n = len(buf)
    while p < n:
        if p + 3 > n:
            raise ValueError(f"trunc@{p}")
        op = buf[p]
        ln = buf[p + 1] | (buf[p + 2] << 8)
        end = p + 3 + ln
        if end > n:
            raise ValueError(f"overrun@{p} op={op:02x} len={ln}")
        out.append((op, ln, buf[p + 3:end]))
        p = end
    return out


def main():
    d = open(sys.argv[1], "rb").read()
    ver, entries = parse_ypf(d)
    scripts = [e for e in entries if e[0].startswith("$ysbin\\yst")
               and e[0].endswith(".ybn") and "list" not in e[0]]
    print(f"[YPF] version={ver} scripts={len(scripts)}")

    key_hist = Counter()
    low_plaus = []
    seg_fail = []          # 非 tag0 槽位分段失败
    tag0_stats = Counter() # tag0: total / empty / prefix_ok / prefix_unknown / seg_ok / seg_fail
    op_count = Counter()
    op_lens = defaultdict(Counter)
    op_opsample = defaultdict(list)   # op -> [(operand bytes hex)]
    tag_count = Counter()
    tag_m = defaultdict(Counter)
    tag_op0 = defaultdict(Counter)
    total_instr = 0
    total_slots = 0

    for e in scripts:
        name = e[0]
        y = read_entry(d, e)
        r = recover_key(y)
        if r is None:
            low_plaus.append((name, "no slots"))
            continue
        key, fails, _testable, slots, content = r
        key_hist[key.hex()] += 1
        if fails > 0:
            low_plaus.append((name, f"seg_fails={fails} key={key.hex()}"))
        total_slots += len(slots)

        # 非 tag0 窗口建立覆盖区间表(用于 tag0 前缀检验)
        non0_ranges = [(off, off + ln) for tag, ln, off in slots
                       if tag != 0 and ln > 0 and off + ln <= len(content)]

        for tag, ln, off in slots:
            tag_count[tag] += 1
            buf = content[off:off + ln]
            if tag == 0:
                tag0_stats["total"] += 1
                if ln == 0:
                    tag0_stats["empty"] += 1
                    continue
                contained = any(a <= off and off + ln <= b for a, b in non0_ranges)
                if contained:
                    tag0_stats["prefix_ok"] += 1
                else:
                    tag0_stats["prefix_unknown"] += 1
                    if tag0_stats["prefix_unknown"] <= 3:
                        print(f"  [tag0 not-contained] {name} slot off={off} len={ln} hex={buf[:24].hex()}")
            try:
                instrs = segment(buf)
                if tag == 0:
                    tag0_stats["seg_ok"] += 1
            except ValueError:
                if tag == 0:
                    tag0_stats["seg_fail"] += 1
                else:
                    seg_fail.append((name, f"tag=0x{tag:08X} off={off} len={ln}"))
                continue
            if instrs:
                tag_op0[tag][instrs[0][0]] += 1
            for op, olen, payload in instrs:
                op_count[op] += 1
                op_lens[op][olen] += 1
                if op == 0x4D and tag in (0x00030000, 0x00030001, 0x00030003):
                    tag_m[tag][bytes(payload)] += 1
                if len(op_opsample[op]) < 6:
                    op_opsample[op].append(payload.hex())
            total_instr += len(instrs)

    print(f"\n[SCAN] slots={total_slots} instructions={total_instr}")
    print(f"[KEY] per-file keys: {dict(key_hist)}")
    print(f"[KEY] low-plausibility files: {len(low_plaus)}")
    for x in low_plaus[:8]:
        print(f"    {x}")
    print(f"\n[SEG] non-tag0 segmentation failures: {len(seg_fail)}")
    for x in seg_fail[:10]:
        print(f"    {x}")
    print(f"[TAG0] {dict(tag0_stats)}")

    print(f"\n[OPS] distinct = {len(op_count)}")
    for op, n in op_count.most_common():
        lens = dict(sorted(op_lens[op].items()))
        samples = op_opsample[op][:4]
        print(f"    0x{op:02x}  x{n:<7} operand_len={lens}  samples={samples}")

    print(f"\n[TAG] distinct tags = {len(tag_count)}")
    for tag, n in sorted(tag_count.items()):
        extra = ""
        if tag_m.get(tag):
            extra = f"  M: {[p[:36] for p, _ in tag_m[tag].most_common(5)]}"
        print(f"    0x{tag:08X} x{n}  first-op: {[(f'0x{o:02x}', c) for o, c in tag_op0[tag].most_common(3)]}{extra}")


if __name__ == "__main__":
    main()
