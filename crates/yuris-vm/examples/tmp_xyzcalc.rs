//! 临时取证:黑盒探测 `es.SP.XYZCALC`(s177 pc1510)的坐标变换。
//!
//! 用法: cargo run -p yuris-vm --example tmp_xyzcalc -- <游戏目录>
//!
//! 方法:boot 后以 push_guest_call 调用 XYZCALC(槽, x, y, …),
//! 读回输出变量(@0x17d8/@0x17d9 等)观察 x/y → 屏幕坐标的映射。

use std::sync::Arc;

use yuris_value::{VarRef, VarSpace, Value};
use yuris_vm::boot::Bootstrap;
use yuris_vm::host::PacFileIndex;
use yuris_vm::VmSuspend;

fn read_int(vm: &yuris_vm::GroupVm, id: u16) -> Option<i64> {
    let r = VarRef { space: VarSpace::At, id };
    match vm.store().get(&r) {
        Ok(Value::Int(n)) => Some(*n),
        Ok(Value::Float(f)) => Some(*f as i64),
        _ => None,
    }
}

fn set_int(vm: &mut yuris_vm::GroupVm, id: u16, v: i64) {
    let r = VarRef { space: VarSpace::At, id };
    let _ = vm.store_mut().set(&r, Value::Int(v));
}

fn main() {
    let dir = std::env::args().nth(1).expect("用法: tmp_xyzcalc <游戏目录>");
    let ypf = std::path::Path::new(&dir).join("pac").join("bn.ypf");
    let booted = Bootstrap {
        ypf_bytes: std::fs::read(&ypf).expect("bn.ypf"),
        name_key: 0xC9,
        key: [0x2b, 0x90, 0x4f, 0x93],
        entry_label: None,
    }
    .boot()
    .expect("boot");
    let mut vm = booted.vm;
    vm.set_file_probe(Arc::new(
        PacFileIndex::scan_game_dir(std::path::Path::new(&dir), 0xC9).expect("pac"),
    ));
    vm.set_strict(false); // 探针:Unsupported 记录后继续(不挂在首个未实现命令)
    for _ in 0..80 {
        match vm.run(4000) {
            Ok(VmSuspend::None) => {}
            _ => break,
        }
    }
    println!("[xyz] boot 后帧深={} pc={}", vm.frame_depth(), vm.pc());
    // 参考状态:屏幕/相机相关配置(存在则打印)
    for id in [0x0460u16, 0x0470, 0x0480, 0x0481] {
        println!("[xyz] @{id:#06x} = {:?}", read_int(&vm, id));
    }
    for (label, slot, x, y) in [
        ("原点", 1, 0, 0),
        ("x=-270", 1, -270, 0),
        ("x=-270,y=114", 1, -270, 114),
        ("x=0,y=0", 1, 0, 0),
        ("x=876,y=162", 1, 876, 162),
        ("x=100,y=200", 1, 100, 200),
    ] {
        // 原生启动填充的屏幕配置(WINDOWINFO oracle: 1920×1080)探针手工补上
        set_int(&mut vm, 0x0460, 1920);
        set_int(&mut vm, 0x0470, 1080);
        set_int(&mut vm, 0x17d3, slot);
        set_int(&mut vm, 0x17dc, x);
        set_int(&mut vm, 0x17dd, y);
        let before = vm.frame_depth();
        let ints: Vec<(u32, i64)> = vec![(1, slot), (2, x), (3, y), (4, 0), (5, 0)];
        if let Err(e) = vm.push_guest_call("es.SP.XYZCALC", &ints, &[]) {
            println!("[{label}] push 失败: {e}");
            continue;
        }
        for _ in 0..50 {
            if vm.frame_depth() < before {
                break;
            }
            match vm.run(2000) {
                Ok(VmSuspend::None) => {}
                Ok(_) => {}
                Err(e) => {
                    println!("[{label}] run 中止: {e}");
                    break;
                }
            }
        }
        println!(
            "[{label}] in=({x},{y}) → @17d8={:?} @17d9={:?} @17da={:?} @17db={:?}",
            read_int(&vm, 0x17d8),
            read_int(&vm, 0x17d9),
            read_int(&vm, 0x17da),
            read_int(&vm, 0x17db)
        );
    }
}
