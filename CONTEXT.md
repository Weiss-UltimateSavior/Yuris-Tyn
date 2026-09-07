# YurisKernel

用 Rust 从零实现的 YU-RIS 引擎兼容运行内核：直接读取原版游戏封包与脚本，在自研内核上运行原游戏。不是提取器、不是反编译器、不是转换器。本文件是全项目唯一术语表，统一文档、代码与讨论中的用语；定义与 `docs/`、`PROGRESS.md` 的定性保持一致。

## 项目与方法

**YurisKernel**:
本项目产物：YU-RIS 引擎兼容运行内核（Rust workspace，17 crates）。
_Avoid_: Yuris-Tyn（那是仓库名）、模拟器（本项目是兼容重实现，非模拟）

**样本游戏**:
AnimalTrailGirlishSquare 2（`kemonomichi2.exe`，引擎 v555 / YPF v500）——所有格式逆向与实机验证的基准语料。

**证据等级**:
每条逆向结论必须标注的四级标签：**Confirmed**（样本实测 + 可复现验证）/ **Likely**（间接证据，实现须加 `// UNVERIFIED` 与对照测试）/ **Hypothesis**（禁止进核心路径）/ **Unknown**（保持 `unimplemented!()`，不猜）。

**勘误**:
被新证据推翻并公开更正的旧结论。必须留痕（旧结论、更正内容、发现方式），不得静默修改。

**对拍**:
正确性验证方法：用 trace 工具从原引擎提取逐组执行流，与本内核 VM 的执行流逐组比对。
_Avoid_: diff 验证、golden 比对

**trace**:
逐组（命令级）执行流的 JSONL 记录。引擎侧 = 从原引擎采集的真值（engine_trace）；VM 侧 = `yuris-vm` 同格式导出（vm_trace）。

**探针（probe）**:
`scripts/` 下的 Python 验证脚本：对样本数据逐条断言，把一个格式结论固化为可复现的验证步骤。
_Avoid_: 测试脚本（Rust 侧另有单元测试）

**成果**:
PROGRESS.md 的记录单元 = 日期 + 阶段 + 结论 + 证据 + 证据等级。PROGRESS.md 是唯一进度真相来源。

## 封包与容器（L0/L1）

**YPF**:
YU-RIS 封包容器（magic `YPF\0`）：条目索引 + 数据区；资源明文 stored、脚本 zlib 压缩，条目名逐字节 XOR name_key。

**name_key**:
YPF 条目名的单字节 XOR 密钥（样本 = 0xC9）。SJIS 名的合法尾字节异或后可能恰为存储态 0x00，名字边界必须结构化校验，不能按首个 0x00 朴素截断。

**bn 型 / se 型条目**:
同一 YPF 包内并存的两种条目布局：bn 型（脚本/文本条目，flag 决定 zlib/stored）与 se 型（资源条目，名字后带内容类型码：0x02=PNG / 0x05=WAV / 0x06=OGG，数据恒 stored）。

**YSTB**:
脚本容器（magic `YSTB`）：header + part1（命令实例表）+ commands（窗口表）+ content（表达式池）+ part4（content 的逻辑延续）。加密 = 4 字节循环 XOR，跳过 header。

**ystb_key**:
YSTB 的 4 字节循环 XOR 密钥，逐游戏不同（样本 = `2b904f93`，游戏内统一）。由首窗口 offset==0 等约束自动猜测（guess-key），不得硬编码进正式路径。

**YSCM**:
引擎命令字典（`ysc.ybn`）：121 条命令的名字、参数名与参数类型码。是命令语义的字典，不是表达式 opcode 表。
_Avoid_: opcode 表

**YSCF**:
编译器配置/键值表（`yscfg.ybn`）。

**YSER**:
错误消息池（`yse.ybn`）：连续 C 串错误文本。
_Avoid_: 资源条目表（README 旧称，已被成果 34 更正）

**YSLB**:
标签表（`ysl.ybn`）：标签名 → murmur2 哈希 + 目标组 PC + 脚本号。GO/GOSUB 跳转查此表。

**YSVR**:
变量定义表（`ysv.ybn`）：变量 id、类别、类型（INT/FLT/STR）、维数边界与初值；启动链据此应用初值。

**YSTD**:
16 字节静态容器（`yst.ybn`），字段语义 Unknown；本引擎版本可能未使用。

**YSSD / SNP**:
存档格式及其 snappy 变体压缩（YSSNP.DLL）。

**YMV**:
影片容器（`mv001~004.ymv`）。

## 脚本与字节码（L2/L3）

**命令（Command）**:
YU-RIS 脚本语言的语句：GO、GOSUB、IF、LET、LOOP、VARACT……由 YSCM 定义，在命令层执行；流程控制全部在此层。
_Avoid_: opcode、指令

**命令组（Group）**:
YSTB 的执行单元：part1 命令实例表的一项（类型 = YSCM 命令下标 + 窗口数 + gparam）。PC = 组下标，引擎"先取后增"。
_Avoid_: 命令实例

**窗口（Window）**:
12 字节记录（tag / len / offset），是共享池中一段字节码的注记。tag 位段：B0 = YSCM 参数下标、B2 = 值类型、B3 = 次级编号。
_Avoid_: 槽位、slot、CommandSlot（旧称）

**参数槽（Param slot）**:
命令执行时按参数下标（tag.B0）映射的求值槽：处理器经槽映射取窗口求值结果。与"窗口"是两个概念，不得混称"槽位"。

**共享池**:
content ‖ part4 拼接成的连续表达式字节码/文本池；窗口 offset 相对此拼接段。
_Avoid_: 字符串区（strs，旧名）、单称 content 区

**顺序型 / 池式脚本**:
脚本两种形态：顺序型窗口无缝覆盖全池（连续性 1.0）；池式窗口重叠（同字节被多窗口引用），tag0 文本窗口常为前缀截断。

**opcode**:
表达式字节码的操作码（30 种，栈机：字面量 / 运算 / 变量访问 / 类型转换），自描述变长编码；不含任何流程控制。
_Avoid_: 命令（那是 YSCM 层概念）

**指令（Instruction）**:
`yuris-script` 把 opcode 字节码译码后的 IR 单元。

**M-串**:
0x4D 前缀的变长控制串（长度 + 引号包裹载荷），承载标签名、UI 串、路径等。

**变量**:
运行期变量没有名字，只有 id；类型（INT/FLT/STR）与维数由 YSVR / 声明命令决定。名字只存在于编译期 ERIS 源码。
_Avoid_: 具名变量

**前缀字符（@ / $）**:
表达式中的变量记号：@ = INT/FLT 变量，$ = STR 变量。

**系统变量（@id）**:
小 id（<1000）系统变量"读即计算、无存储"：如 @48 = 内层 LOOP 迭代计数（1 基，无循环 = 0）、@53 = INT 帧局部数组、@50[x] = 变量 x 当前值。

**帧局部 / 返回值槽**:
GOSUB 压帧携带的局部数组与回写返回值的固定槽区；跨脚本调用整体重绑。

**挂起 / 恢复（suspend / resume）**:
VM 的非阻塞等待模型：WAIT / 文本等待等命令让出执行权，帧节拍或定时器到期后恢复，绝不阻塞线程。

## 运行时与播放器（L4–P7）

**启动链（Bootstrap）**:
内核启动编排：载 YSCM 与命令表 → 分配变量描述符 → 载 YSVR 应用初值 → 查入口标签（SYSTEM_START）→ 建任务加载脚本 → 运行。

**RuntimeApi**:
Kernel 与平台之间的唯一边界：Graphics / Audio / Input / Storage 的 trait。VM 永不直接触碰平台 API。
_Avoid_: 后端直调

**Scene**:
纯状态模型：Layer 树 / 文本 / 动画，不碰任何平台 API。

**SceneBridge**:
把 VM 事件翻译成 Scene 状态变更的桥（CG / CGEND / TEXT → 图层）。

**双脚本系统**:
当前播放器架构：YSTB VM（系统链/控件）与 scenario 播放器并行驱动同一画面。
_Avoid_: 单 VM 全接管（目标态，尚未达成）

**scenario（剧本）**:
sc.ypf 内的明文台词脚本（SJIS/Big5，`scenario*.txt`）：背景/立绘/淡入淡出/双语台词，独立于 YSTB 字节码。
_Avoid_: 脚本（"脚本"专指 YSTB）

**VersionProfile（版本 Profile）**:
按引擎版本分支的兼容性配置点（如 v555 的 YSTB 布局与 content 语义）。跨版本差异必须走 Profile，不得散落硬编码。
