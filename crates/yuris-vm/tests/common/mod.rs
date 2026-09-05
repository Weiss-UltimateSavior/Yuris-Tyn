//! 测试共用工具:合成 YSTB 构造。
use yuris_format::ystb::YstbFile;

/// 构造一个最小 YSTB:header + part1(命令组表) + commands(窗口表) +
/// content + part4,各区独立 `key[i % 4]` XOR(编译器 004160ac 分区模型)。
///
/// 不变式(铁律):`part1_len == 4×组数` 且 `Σcount×12 == command_len`。
pub fn make_ystb(
    key: [u8; 4],
    groups: &[(u8, u8, u16)],               // (command_type, window_count, param)
    windows: &[([u8; 4], u32, u32, &[u8])], // (tag, len, offset, content)
) -> Vec<u8> {
    fn xor_region(region: &[u8], key: [u8; 4]) -> Vec<u8> {
        region.iter().enumerate().map(|(i, b)| b ^ key[i % 4]).collect()
    }
    assert_eq!(groups.iter().map(|g| g.1 as usize).sum::<usize>(), windows.len());

    let content: Vec<u8> = windows
        .iter()
        .flat_map(|(_, _, _, prog)| prog.iter().copied())
        .collect();

    // 每窗口的 (tag, len, offset) —— len/offset 按上述 content 布局重算
    let mut cmds = Vec::new();
    let mut off = 0u32;
    for (tag, len, base_off, prog) in windows {
        cmds.extend_from_slice(tag);
        cmds.extend_from_slice(&len.to_le_bytes());
        cmds.extend_from_slice(&(off + base_off).to_le_bytes());
        off += prog.len() as u32;
        let _ = *base_off; // base_off 仅用于占位说明;实际由布局决定
    }

    let mut part1 = Vec::new();
    for (t, c, p) in groups {
        part1.push(*t);
        part1.push(*c);
        part1.extend_from_slice(&p.to_le_bytes());
    }

    let p1 = xor_region(&part1, key);
    let cm = xor_region(&cmds, key);
    let ct = xor_region(&content, key);
    let p4 = xor_region(&[0u8; 4], key);

    let mut out = Vec::new();
    out.extend_from_slice(b"YSTB");
    out.extend_from_slice(&555u32.to_le_bytes());
    out.extend_from_slice(&(groups.len() as u32).to_le_bytes());
    out.extend_from_slice(&(p1.len() as u32).to_le_bytes());
    out.extend_from_slice(&(cm.len() as u32).to_le_bytes());
    out.extend_from_slice(&(ct.len() as u32).to_le_bytes());
    out.extend_from_slice(&(p4.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&p1);
    out.extend_from_slice(&cm);
    out.extend_from_slice(&ct);
    out.extend_from_slice(&p4);
    out
}


/// 常用 key(样本实测)。
pub const SAMPLE_KEY: [u8; 4] = [0x2b, 0x90, 0x4f, 0x93];

/// 占位 tag(len/offset 由 make_ystb 布局重算)。
pub const DUMMY: [u8; 4] = [0; 4];
