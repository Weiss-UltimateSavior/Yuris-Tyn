//! 临时诊断:启动链 LOOPEND 配对问题(打印事件流)。

use yuris_vm::{ResumeResponse, VmSuspend, VmEvent};

#[test]
fn diag_bootstrap_event_flow() {
    let p = std::path::PathBuf::from(
        "/Users/weiss/Desktop/yuris/AnimalTrailGirlishSquare 2/pac/bn.ypf",
    );
    if !p.exists() {
        eprintln!("skip");
        return;
    }
    let bytes = std::fs::read(&p).unwrap();
    let boot = yuris_vm::boot::Bootstrap {
        ypf_bytes: bytes,
        name_key: 0xC9,
        key: [0x2b, 0x90, 0x4f, 0x93],
        entry_label: None,
    };
    let booted = boot.boot().unwrap();
    let mut vm = booted.vm;
    let mut last = 0usize;
    // 主循环每帧 WAIT(FRAME=1);无后端环境以固定组数预算为界
    let budget = 2_000_000usize;
    loop {
        if vm.executed_groups() >= budget {
            eprintln!("达到预算 {budget}");
            break;
        }
        match vm.run(200) {
            Ok(VmSuspend::None) => continue,
            Ok(VmSuspend::Complete) => break,
            Ok(VmSuspend::Wait { .. }) => {
                vm.resume(ResumeResponse::Continue).unwrap();
                continue;
            }
            Ok(VmSuspend::Error(msg)) => {
                eprintln!("STOP: {msg}");
                break;
            }
            Err(e) => {
                eprintln!("RUN-ERR: {e}");
                eprintln!("last events:");
                for ev in vm.events().iter().rev().take(12).collect::<Vec<_>>().iter().rev() {
                    match ev {
                        VmEvent::GroupExecuted { pc, command, condition } =>
                            eprintln!("  g{pc} cmd=0x{command:02x} cond={condition:?}"),
                        VmEvent::Jump { from, to, kind } =>
                            eprintln!("  JUMP g{from}→g{to} ({kind:?})"),
                        VmEvent::Call { from, to } =>
                            eprintln!("  CALL g{from}→g{to}"),
                        VmEvent::Unsupported { pc, command, reason } =>
                            eprintln!("  UNSUP g{pc} cmd=0x{command:02x}: {reason}"),
                        VmEvent::ScriptSwitch { from_script, to_script, .. } =>
                            eprintln!("  SWITCH {from_script}→{to_script}"),
                        _ => {}
                    }
                }
                break;
            }
        }
    }
    eprintln!("executed={last}");
    for e in vm.events().iter().rev().take(60).collect::<Vec<_>>().iter().rev() {
        match e {
            VmEvent::GroupExecuted { pc, command, condition } =>
                eprintln!("  g{pc} cmd=0x{command:02x} cond={condition:?}"),
            VmEvent::Jump { from, to, kind } =>
                eprintln!("  JUMP g{from}→g{to} ({kind:?})"),
            VmEvent::Call { from, to } =>
                eprintln!("  CALL g{from}→g{to}"),
            VmEvent::Unsupported { pc, command, reason } =>
                eprintln!("  UNSUP g{pc} cmd=0x{command:02x}: {reason}"),
            VmEvent::ScriptSwitch { from_script, to_script, .. } =>
                eprintln!("  SWITCH {from_script}→{to_script}"),
            _ => {}
        }
    }
}
