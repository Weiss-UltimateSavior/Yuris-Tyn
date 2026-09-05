# -*- coding: utf-8 -*-
"""分析 bt_probe.log:每个搜索是否命中。"""
import re

lines = [l.strip() for l in open(r"C:\Users\weiss\AppData\Local\Temp\bt_probe.log", encoding="utf-8") if l.strip()]
searches = 0
found = 0
last_i = None
prev = None
fails = []
for l in lines:
    m = re.match(r'g30 i=(\d+) search="(.*)" tbl="(.*)" cnt="int(\d+)"', l)
    if m:
        i, s, t = int(m.group(1)), m.group(2), m.group(3)
        last_i = i
        if s == t:
            found += 1
        continue
    m2 = re.match(r'g27 search_str="(.*)"', l)
    if m2:
        if searches > 0 and found == 0:
            fails.append((prev, last_i))
        searches += 1
        prev = m2.group(1)
        found = 0
if searches > 0 and found == 0:
    fails.append((prev, last_i))
print("searches:", searches, "matched:", searches - len(fails), "failed:", len(fails))
for f in fails:
    print("FAIL:", f)
