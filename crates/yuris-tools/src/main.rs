//! `yuris` —— YU-RIS 格式检查工具（开发向，无 GUI）。
//!
//! 子命令：
//!
//! ```text
//! yuris ypf list     <bn.ypf> [--limit N]
//! yuris ypf extract  <bn.ypf> (--name NAME | --index N) [-o OUT]
//! yuris ystb guess-key <bn.ypf> (--name NAME | --index N)
//! yuris ystb info    <bn.ypf> (--name NAME | --index N) [--key-hex HEX]
//! yuris ystb slots   <bn.ypf> (--name NAME | --index N) [--key-hex HEX] [--limit N]
//! yuris yscf         <bn.ypf> --name '$ysbin\yscfg.ybn'
//! ```
//!
//! 所有命令都基于 `docs/formats/` 中 **Confirmed** 的规格实现。

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use yuris_core::VersionProfile;
use yuris_format::{ypf::YpfArchive, yscf::YscfFile, ystb::{guess_key, YstbFile}};

#[derive(Parser)]
#[command(name = "yuris", version, about = "YU-RIS format inspector")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// YPF 封包操作。
    Ypf {
        #[command(subcommand)]
        sub: YpfCmd,
    },
    /// YSTB 脚本容器操作。
    Ystb {
        #[command(subcommand)]
        sub: YstbCmd,
    },
    /// YSCM 命令字典（脚本命令表）。
    Yscm {
        /// bn.ypf 路径。
        path: PathBuf,
        /// 条目名（默认 `%ysbin\ysc.ybn`）。
        #[arg(long)]
        name: Option<String>,
        /// 条目下标。
        #[arg(long)]
        index: Option<usize>,
        /// 只列出命令名与参数个数。
        #[arg(long)]
        list: bool,
        /// 只显示某个命令的参数表。
        #[arg(long)]
        cmd: Option<String>,
    },
    /// YSCF 工程配置。
    Yscf {
        /// bn.ypf 路径。
        path: PathBuf,
        /// 条目名（如 `$ysbin\yscfg.ybn`），或用 --index。
        #[arg(long)]
        name: Option<String>,
        /// 条目下标。
        #[arg(long)]
        index: Option<usize>,
    },
}

#[derive(Subcommand)]
enum YpfCmd {
    /// 列出全部条目。
    List {
        /// bn.ypf 路径。
        path: PathBuf,
        /// 只显示前 N 条。
        #[arg(long)]
        limit: Option<usize>,
        /// 文件名 XOR key（默认 0xC9，仅本样本验证）。
        #[arg(long)]
        key: Option<u8>,
    },
    /// 提取条目。
    Extract {
        /// bn.ypf 路径。
        path: PathBuf,
        /// 条目名。
        #[arg(long)]
        name: Option<String>,
        /// 条目下标。
        #[arg(long)]
        index: Option<usize>,
        /// 输出文件（缺省打到 stdout）。
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum YstbCmd {
    /// 自动猜测 XOR 密钥。
    GuessKey {
        /// bn.ypf 路径。
        path: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        index: Option<usize>,
    },
    /// 打印 header 与槽位统计。
    Info {
        /// bn.ypf 路径。
        path: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        index: Option<usize>,
        /// 4 字节密钥（hex，8 个字符）。
        #[arg(long)]
        key_hex: Option<String>,
    },
    /// 打印槽位表。
    Slots {
        /// bn.ypf 路径。
        path: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        index: Option<usize>,
        #[arg(long)]
        key_hex: Option<String>,
        /// 只显示前 N 条。
        #[arg(long, default_value_t = 24)]
        limit: usize,
    },
    /// 反汇编 content 窗口为指令(已证实语义;未知保留 raw)。
    Disasm {
        /// bn.ypf 路径。
        path: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        index: Option<usize>,
        #[arg(long)]
        key_hex: Option<String>,
        /// 只显示前 N 个窗口。
        #[arg(long, default_value_t = 12)]
        limit: usize,
    },
}

fn resolve_name(ypf: &YpfArchive, name: &Option<String>, index: &Option<usize>) -> Result<String> {
    if let Some(n) = name {
        return Ok(n.clone());
    }
    if let Some(i) = index {
        return ypf
            .entries()
            .get(*i)
            .map(|e| e.name.clone())
            .with_context(|| format!("index {i} out of range"));
    }
    bail!("need --name or --index");
}

fn open_ypf(path: &PathBuf, name_key: u8) -> Result<YpfArchive> {
    let data = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    YpfArchive::from_bytes(data, name_key).map_err(Into::into)
}

fn get_script(ypf: &YpfArchive, name: &str) -> Result<Vec<u8>> {
    ypf.read(name).with_context(|| format!("read entry {name}"))
}

fn parse_key(hex: &str) -> Result<[u8; 4]> {
    let b = hex::decode(hex).context("key must be hex")?;
    if b.len() != 4 {
        bail!("key must be exactly 4 bytes (8 hex chars)");
    }
    Ok([b[0], b[1], b[2], b[3]])
}

/// 简易 hex 编码（避免额外依赖）。
mod hex {
    /// 解码 4 字节 hex。
    pub fn decode(s: &str) -> anyhow::Result<Vec<u8>> {
        let s = s.trim();
        if s.len() % 2 != 0 {
            anyhow::bail!("odd hex length");
        }
        let mut out = Vec::with_capacity(s.len() / 2);
        for i in 0..s.len() / 2 {
            out.push(u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)?);
        }
        Ok(out)
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let profile = VersionProfile::sample_v555();

    match cli.cmd {
        Cmd::Ypf { sub } => match sub {
            YpfCmd::List { path, limit, key } => {
                let ypf = open_ypf(&path, key.unwrap_or(profile.ypf_name_xor_key))?;
                let h = ypf.header();
                println!(
                    "version={} count={} first_data_off={:#x} index_prefix={:#x}",
                    h.version,
                    h.file_count,
                    h.first_data_off,
                    ypf.index_prefix()
                );
                let entries = ypf.entries();
                let n = limit.unwrap_or(entries.len());
                println!("{:>4}  {:<24} {:>5} {:>9} {:>9} {:>9}", "#", "name", "flag", "uncomp", "comp", "offset");
                for (i, e) in entries.iter().take(n).enumerate() {
                    println!(
                        "{:>4}  {:<24} {:>5} {:>9} {:>9} {:>9}",
                        i,
                        e.name.replace('\\', "/"),
                        e.flags,
                        e.uncompressed_len,
                        e.compressed_len,
                        format!("{:#x}", e.offset)
                    );
                }
            }
            YpfCmd::Extract { path, name, index, out } => {
                let ypf = open_ypf(&path, profile.ypf_name_xor_key)?;
                let name = resolve_name(&ypf, &name, &index)?;
                let data = get_script(&ypf, &name)?;
                match out {
                    Some(p) => {
                        std::fs::write(&p, &data)?;
                        println!("wrote {} bytes -> {}", data.len(), p.display());
                    }
                    None => {
                        use std::io::Write;
                        std::io::stdout().write_all(&data)?;
                    }
                }
            }
        },
        Cmd::Ystb { sub } => match sub {
            YstbCmd::GuessKey { path, name, index } => {
                let ypf = open_ypf(&path, profile.ypf_name_xor_key)?;
                let name = resolve_name(&ypf, &name, &index)?;
                let blob = get_script(&ypf, &name)?;
                match guess_key(&blob) {
                    Ok((k, score)) => println!(
                        "key = {}  score = {:.4}  (key_score = max(contiguity, window-closure), 1.0 = perfect)",
                        k.iter().map(|b| format!("{b:02x}")).collect::<String>(),
                        score
                    ),
                    Err(e) => bail!("guess failed: {e}"),
                }
            }
            YstbCmd::Info { path, name, index, key_hex } => {
                let ypf = open_ypf(&path, profile.ypf_name_xor_key)?;
                let name = resolve_name(&ypf, &name, &index)?;
                let blob = get_script(&ypf, &name)?;
                let key = match key_hex {
                    Some(h) => parse_key(&h)?,
                    None => guess_key(&blob)?.0,
                };
                let f = YstbFile::from_bytes(&blob, key)?;
                let h = f.header();
                println!("key = {}", kstr(&key));
                println!(
                    "version={} unknown1={} unknown2={}",
                    h.version, h.unknown1, h.unknown2
                );
                println!(
                    "part1={} command={} content={} part4={}",
                    h.part1_len, h.command_len, h.content_len, h.part4_len
                );
                println!("slots={} contiguity={:.4}", f.slots().len(), f.contiguity_score());
                let mut hist: std::collections::BTreeMap<u32, usize> = Default::default();
                for s in f.slots() {
                    *hist.entry(s.tag).or_default() += 1;
                }
                println!("tag histogram:");
                for (t, n) in hist {
                    println!("  0x{t:08X}  x{n}");
                }
            }
            YstbCmd::Slots { path, name, index, key_hex, limit } => {
                let ypf = open_ypf(&path, profile.ypf_name_xor_key)?;
                let name = resolve_name(&ypf, &name, &index)?;
                let blob = get_script(&ypf, &name)?;
                let key = match key_hex {
                    Some(h) => parse_key(&h)?,
                    None => guess_key(&blob)?.0,
                };
                let f = YstbFile::from_bytes(&blob, key)?;
                println!("{:>4}  {:<12} {:>8} {:>8}", "#", "tag", "len", "offset");
                for (i, s) in f.slots().iter().take(limit).enumerate() {
                    println!(
                        "{:>4}  0x{:<10} {:>8} {:>8}",
                        i,
                        format!("{:08X}", s.tag),
                        s.len,
                        s.offset
                    );
                }
            }
            YstbCmd::Disasm { path, name, index, key_hex, limit } => {
                let ypf = open_ypf(&path, profile.ypf_name_xor_key)?;
                let name = resolve_name(&ypf, &name, &index)?;
                let blob = get_script(&ypf, &name)?;
                let key = match key_hex {
                    Some(h) => parse_key(&h)?,
                    None => guess_key(&blob)?.0,
                };
                let f = YstbFile::from_bytes(&blob, key)?;
                println!("key = {}", kstr(&key));
                let mut shown = 0usize;
                let mut unknown_ops: std::collections::BTreeMap<u8, usize> = Default::default();
                for (i, s) in f.slots().iter().enumerate() {
                    if shown >= limit {
                        break;
                    }
                    let Ok(w) = f.slot_content(s) else {
                        println!("slot #{i:<4} tag=0x{:08X} off={:<6} len={:<4} <越界特殊记录>", s.tag, s.offset, s.len);
                        shown += 1;
                        continue;
                    };
                    let instrs = yuris_script::decode_window(w)
                        .with_context(|| format!("slot #{i}"))?;
                    println!(
                        "slot #{i:<4} tag=0x{:08X} off={:<6} len={:<4} instrs={}",
                        s.tag,
                        s.offset,
                        s.len,
                        instrs.len()
                    );
                    for ins in instrs {
                        let detail = match &ins.kind {
                            yuris_script::Insn::PushInt(v) => format!("{v}"),
                            yuris_script::Insn::PushFloat(v) => format!("{v}"),
                            yuris_script::Insn::PushStr(p) => {
                                format!("{:?}", String::from_utf8_lossy(p))
                            }
                            yuris_script::Insn::PushVar(r)
                            | yuris_script::Insn::PushVarRef(r)
                            | yuris_script::Insn::PushVarIndexed(r) => {
                                let tag = match r.space {
                                    yuris_script::VarSpace::At => "@",
                                    yuris_script::VarSpace::Dollar => "$",
                                    yuris_script::VarSpace::Hash => "#",
                                    yuris_script::VarSpace::Backtick => "`",
                                    yuris_script::VarSpace::Unknown(_) => "?",
                                };
                                format!("{}{}", tag, r.id)
                            }
                            yuris_script::Insn::Unknown { raw_op, operand } => {
                                *unknown_ops.entry(*raw_op).or_default() += 1;
                                format!(
                                    "op={:#04x} operand={}",
                                    raw_op,
                                    operand.iter().map(|b| format!("{b:02x}")).collect::<String>()
                                )
                            }
                            _ => String::new(),
                        };
                        println!(
                            "    +{:<4} {:<11} {}",
                            ins.offset,
                            ins.kind.mnemonic(),
                            detail
                        );
                    }
                    shown += 1;
                }
                if !unknown_ops.is_empty() {
                    println!("unknown opcodes: {unknown_ops:?}  (见 docs/opcode/opcode-table.md §未解清单)");
                }
            }
        },
        Cmd::Yscm { path, name, index, list, cmd } => {
            let ypf = open_ypf(&path, profile.ypf_name_xor_key)?;
            let entry_name = match (name, index) {
                (Some(n), _) => n,
                (None, Some(i)) => resolve_name(&ypf, &None, &Some(i))?,
                (None, None) => "%ysbin\\ysc.ybn".to_string(),
            };
            let blob = get_script(&ypf, &entry_name)?;
            let t = yuris_format::yscm::YscmTable::from_bytes(&blob)
                .with_context(|| format!("parse {entry_name}"))?;
            println!(
                "version={} commands={} params={} body_end={:#x} tail={}",
                t.version(),
                t.commands().len(),
                t.param_total(),
                t.tail_offset,
                t.tail.len()
            );
            if let Some(c) = &cmd {
                let c = t
                    .command(c)
                    .with_context(|| format!("command {c} not found"))?;
                println!("{} ({} params)", c.name, c.params.len());
                for p in &c.params {
                    println!("  {:<14} type=0x{:04X} ({})", p.name, p.ty, p.ty);
                }
            } else if list {
                for c in t.commands() {
                    println!("{:<14} {}", c.name, c.params.len());
                }
            } else {
                for c in t.commands() {
                    let ps: Vec<String> = c
                        .params
                        .iter()
                        .map(|p| format!("{}:{}", p.name, p.ty))
                        .collect();
                    println!("[{:3}] {:<14} {}", 0, c.name, ps.join(","));
                }
            }
        }
        Cmd::Yscf { path, name, index } => {
            let ypf = open_ypf(&path, profile.ypf_name_xor_key)?;
            let name = resolve_name(&ypf, &name, &index)?;
            let blob = get_script(&ypf, &name)?;
            let f = YscfFile::from_bytes(&blob)?;
            println!("version={}", f.version);
            println!("screen = {}x{}", f.screen_width, f.screen_height);
            println!(
                "file_priority: dev={} debug={} release={}  (unpacked read = {})",
                f.file_priority_dev,
                f.file_priority_debug,
                f.file_priority_release,
                if f.unpacked_read_enabled() { "ENABLED" } else { "disabled" }
            );
            println!("image_type_slots = {:?}", f.image_type_slots);
            println!("sound_type_slots = {:?}", f.sound_type_slots);
            println!("caption = {:?}", f.caption);
        }
    }
    Ok(())
}

fn kstr(k: &[u8; 4]) -> String {
    k.iter().map(|b| format!("{b:02x}")).collect()
}
