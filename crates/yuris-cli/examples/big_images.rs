//! 按钮纹理尺寸普查。
fn main() {
    let path = "AnimalTrailGirlishSquare 2/pac/cgsys_ec.ypf";
    let index = yuris_format::ypf::YpfIndex::from_path(std::path::Path::new(path), 0xC9).unwrap();
    use std::io::{Read, Seek, SeekFrom};
    for e in &index.entries {
        let name = String::from_utf8_lossy(&e.name).into_owned();
        let ln = name.to_ascii_lowercase();
        if !(ln.contains("btn_start") || ln.contains("btn_load") || ln.contains("btn_lastload")
            || ln.contains("btn_end") || ln.contains("btn_extra"))
        {
            continue;
        }
        let mut f = std::fs::File::open(path).unwrap();
        f.seek(SeekFrom::Start(e.offset as u64)).unwrap();
        let mut head = [0u8; 24];
        if f.read_exact(&mut head).is_err() || head[..8] != *b"\x89PNG\r\n\x1a\n" {
            continue;
        }
        let w = u32::from_be_bytes(head[16..20].try_into().unwrap());
        let h = u32::from_be_bytes(head[20..24].try_into().unwrap());
        println!("{} {}x{}", name.trim(), w, h);
    }
}
