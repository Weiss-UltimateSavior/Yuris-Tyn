# Yuris-Tyn

> **用 Rust 从零实现一个 YU-RIS 引擎兼容运行内核:直接读取原始游戏数据并运行原游戏。**
> 不是提取器,不是反编译器,不是转换器。

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#许可)

## 项目简介

本项目通过对引擎二进制的逆向分析,以纯 Rust 重新实现其运行时内核:解析 YPF 封包、解码 YSTB 字节码、
逐组解释执行,并通过 winit + wgpu 渲染画面,目标是让原游戏在自研内核上完整运行。
使用与测试的样本游戏为：AnimalTrailGirlishSquare 2

**实机状态目前非常糟糕，虽然可以运行但是 存在包括但不限于 UI控件定位以及素材叠层定位等其他许多问题。
属于半成品状态，希望能有大佬接手或者参考,继续推进该引擎的跨平台运行可行性。
没办法，token与精力不足支持项目的继续（免费鸡蛋的极限，原谅我没有钱去购买token）
逆向工程与代码实现均由zcode免费发放的5.3f与TraeWork签到免费积分5.3f产出

**当前状态:M1(出画面)与 M2(首个可玩切片)已达成** ——
内核可从封包直接启动游戏:厂商 LOGO → 注意事项页 → 分层标题画面 →
进入第一幕对话(BGM 循环、voice 逐行播放、立绘按脚本出场、点击推进台词、
选择肢分支、F5/F9 快速存读档)。

**正确性验证方式(对拍)**:通过自研的引擎 trace 采集工具,从原引擎提取
逐命令执行流(335,069 组),与本内核 VM 的独立执行流逐组比对 ——
**对齐区间内唯一 1 处分歧**(已定性:输入子系统未实现,属已知缺口)。

## 架构

Cargo workspace,16 个 crate 按层次划分:

| 层 | Crate | 职责 |
|---|---|---|
| L0/L1 | `yuris-format` | YPF 封包 / YSTB / YSCF / YSCM / YSER / YSSD+SNP 格式解码 |
| L2 | `yuris-script` | 变长 VM 字节码 → `Instruction` IR |
| L3 | `yuris-vm` | 组级执行 VM:PC、帧、GOSUB/LOOP、系统变量、挂起恢复 |
| L3 | `yuris-expr` | 表达式引擎(Token → AST → Evaluator) |
| L3 | `yuris-value` | INT/FLT/STR 三类型语义与变量存储 |
| L4 | `yuris-runtime` | RuntimeApi / Graphics / Audio / Input / Storage 后端 trait |
| L5 | `yuris-scene` | Scene / Layer / Animation 纯状态模型 |
| L6 | `yuris-render` | winit 0.30 + wgpu 24 渲染后端(图层合成、letterbox、文本栅格化) |
| L6 | `yuris-resource` | 多包挂载、松散根优先、seek 式随机读取、PNG/OGG 解码链路 |
| L6 | `yuris-audio` / `yuris-input` / `yuris-save` | 音频 / 输入 / 存档(骨架) |
| P7 | `yuris-scenario` | 明文剧本(scenario\*.txt)词法/行解析器 |
| — | `yuris-scenario` 双脚本系统:YSTB VM + scenario 播放器并行驱动 | |
| — | `yuris-core` | 错误类型、字节工具、版本 Profile(地基) |
| — | `yuris-cli` | 播放器入口(`yuris-cli run <游戏目录>`) |
| — | `yuris-tools` | 开发向 CLI:ypf / ystb / trace / opcode-scan |

## 已逆向的格式(均为实测 Confirmed)

| 格式 | 内容 |
|---|---|
| **YPF** | 封包容器。名字 XOR 0xC9;bn/se 两种条目布局条目级自适应并存;资源明文 stored、脚本 zlib 压缩 |
| **YSTB** | 脚本容器。4 分区 + 4 字节循环 XOR(密钥逐样本定位);命令区定长 12B 记录 |
| **YSCF** | 编译器配置/键值表 |
| **YSCM** | 引擎命令槽位定义(命令名 → 参数槽名表) |
| **YSER** | 资源条目表 |
| **YSSD + SNP** | 存档格式;SNP = snappy 变体(YSSNP.DLL 全逆向:字面量长度 +1、copy 族与标准一致) |
| **op.ypf** | 实为 ASF/WMV 视频(OP 动画),非加密封包 |
| 资源内容 | 图像 = 标准 PNG、音频 = 标准 OGG(可直接用 image / symphonia 解码) |

详细格式规格见 `docs/formats/`,命令语义见 `docs/engine/command-layer.md`,
指令编码见 `docs/opcode/opcode-table.md`。

## 构建与运行

```text
工具链:Rust (MSVC),edition 2021

cargo build --workspace          # 构建
cargo test --workspace           # 47 个测试套件

# 运行(需要自备正版游戏,指向游戏安装目录):
cargo run --release -p yuris-cli -- run "<游戏目录>"
# 调试跳转到指定剧本标签:
cargo run --release -p yuris-cli -- run "<游戏目录>" --at "maho2_22"
```

操作:点击推进对话 / 选择肢;F5 快速存档,F9 快速读档,Esc 退出。

## 目录结构

```text
├── crates/            # 16 个 Rust crate(workspace members)
├── docs/              # 架构设计、格式规格、命令语义、opcode 表
│   ├── formats/       #   YPF/YSTB/YSCM/YSER 格式文档
│   ├── engine/        #   命令层语义(含系统变量 case 表)
│   └── opcode/        #   指令编码表
├── scripts/           # Python 逆向/探针工具(格式侦查、trace 采集、对拍 diff)
│   └── ghidra/        #   PyGhidra 自动化反编译脚本
├── PROGRESS.md        # 开发进度日志(唯一进度真相来源:69 项成果、证据等级)
├── Cargo.toml         # workspace 清单
└── README.md
```

## 方法论:证据等级驱动的逆向

本项目的核心纪律是**不猜**。每个结论都标注证据等级并记录在 `PROGRESS.md`:

| 等级 | 含义 |
|---|---|
| **Confirmed** | 有实测数据 + 可复现验证步骤,直接写实现与单测 |
| **Likely** | 有间接证据,实现须加 `// UNVERIFIED` 与对照测试 |
| **Hypothesis** | 结构推断,禁止写进核心路径 |
| **Unknown** | 显式 `unimplemented!()`,绝不猜测 |

`PROGRESS.md` 完整记录了 69 项成果的推导过程、验证命令与历次勘误,
是理解本项目的最佳入口。

## 版权与免责声明

- 本仓库**不包含任何游戏资源、游戏数据或引擎二进制**(封包、CG、音频、
  可执行文件均不入库;由引擎二进制反编译产生的代码同样不入库)。
- 本项目是原创的引擎兼容实现,仅供**互操作性研究与学习**。
  运行游戏需要你**自行持有正版游戏副本**。
- YU-RIS 及游戏作品版权归各自权利人所有,与本项目无关。

## 许可

本项目采用 [GNU General Public License v3.0 或更高版本](LICENSE)授权(GPL-3.0-or-later,见 `Cargo.toml`)。
