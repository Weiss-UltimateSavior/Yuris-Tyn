//! 临时取证工具:boot 后扫描变量 store 中已声明数组的边界(全局槽取证,成果 78)。
//!
//! 用法: cargo run -p yuris-vm --example tmp_probe_globals -- <游戏目录>

use yuris_vm::boot::Bootstrap;

fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let ypf = std::path::Path::new(&dir).join("pac").join("bn.ypf");
    let bytes = std::fs::read(&ypf).unwrap();
    let booted = Bootstrap {
        ypf_bytes: bytes,
        name_key: 0xC9,
        key: [0x2b, 0x90, 0x4f, 0x93],
        entry_label: None,
    }
    .boot()
    .expect("boot");
    let store = booted.vm.store();
    println!("--- @ 数组(id, dims, elem) ---");
    for id in 0..=256u16 {
        let r = yuris_value::VarRef {
            space: yuris_value::VarSpace::At,
            id,
        };
        if let Some(dims) = store.array_dims(&r) {
            println!(
                "@{id}: dims={dims:?} elem={:?}",
                store.array_elem_type(&r)
            );
        }
    }
    println!("--- $ 数组 ---");
    for id in 0..=64u16 {
        let r = yuris_value::VarRef {
            space: yuris_value::VarSpace::Dollar,
            id,
        };
        if let Some(dims) = store.array_dims(&r) {
            println!("${id}: dims={dims:?}");
        }
    }
}
