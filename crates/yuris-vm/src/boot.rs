//! 启动链编排器(P0 第3项)。
//!
//! 引擎启动链(成果 25,FUN_0046b63c,Confirmed):
//! `YSCM 命令表 → 变量描述符分配 → 载 YSVR → 应用初始值 → 哈希查入口标签
//! → 建任务 → 加载脚本 → 运行`。
//!
//! 本模块把现有件串联:[`yuris_format::ypf::YpfArchive`](格式化解析) +
//! [`YslbTable`](标签表查找入口) + [`YpfScriptHost`](加载目标脚本) +
//! [`GroupVm`](执行)。入口标签由调用方给定(缺省从 YSLB 找 `SYSTEM_START`)。
//!
//! **铁律**:Unknown 不猜;各步若缺证据则返回 `Error::Unimplemented`。

use std::collections::HashMap;

use yuris_core::{Error, Result};
use yuris_format::yslb::YslbTable;

use crate::GroupVm;
use yuris_value::Value;
use crate::host::{ScriptHost, YpfScriptHost};

/// 启动配置。
pub struct Bootstrap {
    /// 脚本封包字节(bn.ypf)。
    pub ypf_bytes: Vec<u8>,
    /// 封包文件名 XOR key(样本 0xC9)。
    pub name_key: u8,
    /// 脚本条目 XOR 密钥(如 `2b904f93`)。
    pub key: [u8; 4],
    /// 执行入口标签名(缺省从 YSLB 查 `SYSTEM_START`)。
    pub entry_label: Option<Vec<u8>>,
}

/// 启动结果:已初始化的 VM(尚未 run)。
pub struct Booted {
    /// 就绪的 GroupVm(已绑定 Host + 标签表 + 入口脚本)。
    pub vm: GroupVm,
    /// 当前脚本号(入口脚本)。
    pub script_id: u16,
    /// 入口组下标。
    pub entry_pc: u32,
    /// 已应用的 YSVR 条目数(若提供了 YSLB/YSVR,否则 0 —— 见 [`Bootstrap::boot`])。
    pub applied_ysvr: usize,
}

impl Bootstrap {
    /// 编排启动链,返回就绪的 [`GroupVm`]。
    ///
    /// - 从 `ypf` 读 YSLB+脚本;若提供了标签表/初值表则一并接入。
    /// - 缺省入口标签 = `SYSTEM_START`。
    pub fn boot(self) -> Result<Booted> {
        let mut host = YpfScriptHost::from_ypf_bytes(self.ypf_bytes.clone(), self.name_key, self.key)?;
        let yslb_data = host.read_entry("%ysbin\\ysl.ybn")?;
        let yslb = YslbTable::from_bytes(&yslb_data)?;

        // 入口标签名
        let entry: &[u8] = self.entry_label.as_deref().unwrap_or(b"SYSTEM_START");
        let entry_idx = yslb.find(entry).ok_or_else(|| {
            Error::format(format!("入口标签不存在: {:?}", String::from_utf8_lossy(entry)))
        })?;
        let label = &yslb.labels()[entry_idx];
        let (entry_pc, script_id) = (label.target_pc, label.script_id);

        // 预读 YSVR(在 host move 进 Box 前完成所有借用)
        let ysvr_data = host.read_entry("%ysbin\\ysv.ybn")?;
        let ysvr = yuris_format::ysvr::YsvrTable::from_bytes(&ysvr_data)?;

        let ctx = host.load(script_id)?;

        // 构建单脚本 VM(装载入口脚本)
        let mut vm = GroupVm::load(ctx.script)?;
        vm.set_script_id(script_id);

        // 注入标签表(name → (target_pc, script_id))
        let mut labels: HashMap<Vec<u8>, (u32, u16)> = HashMap::new();
        for l in yslb.labels() {
            labels.insert(l.name.clone(), (l.target_pc, l.script_id));
        }
        vm.set_labels(labels);

        // 全量声明消费:引擎变量描述符表全局共享,启动时消费全部脚本的
        // 标量声明组(INT/FLT/STR;端到端证据:未消费则启动链读 @1417/@6292 报
        // 「未定义变量」)。声明在前,YSVR 初值在后。(host move 前完成)
        // 消费后**标记已消费**:运行期 switch_script 不得重消费 —— 重消费会
        // 用声明期边界(expr+1)重建数组,覆盖 YSVR 终态(如 s47 $2729 YSVR
        // bounds=[1] 被刷回 [2],按钮表整体错位一槽;成果 59)。
        {
            let mut n = 0usize;
            for sid in host.script_ids()? {
                let ctx = host.load(sid)?;
                n += crate::consume_declarations_into(vm.store_mut(), &ctx)?;
                vm.mark_declarations_consumed(sid);
            }
            let _ = n;
        }

        // 绑定 host(此后跨脚本可用)
        vm.set_host(Box::new(host));

        // 应用 YSVR 全局初值(启动链「应用初值」步;须在声明之后,否则覆盖初值)。
        // kind1+kind3 在此应用;kind2 暂存,由入口脚本/switch_script 首载应用。
        let applied_ysvr = vm.apply_ysvr(&ysvr)?;
        vm.apply_ysvr_for_script(script_id)?;

        // 注:$1045(存档目录串)引擎侧 = $105 = YSVR 空串(引擎经 VFS 原生
        // 前缀探测命中 save/*.sd,与 $1045 无关;成果 59e)—— 不在此种子。

        // 入口组:引擎先取后增,直接设入口组
        vm.set_pc(entry_pc as usize);

        Ok(Booted {
            vm,
            script_id,
            entry_pc,
            applied_ysvr,
        })
    }
}

impl GroupVm {
    /// 事件流推进到首个 Wait/Complete。供启动链驱动。
    pub fn set_pc(&mut self, pc: usize) {
        self.pc = pc;
    }

    /// P9.2 输入注入(播放器):按键名写入引擎输入串 `$55[1]`。
    ///
    /// `$55` 属帧系统变量族(成果 52:系统族无全局回退)——键链
    /// (s190 `STR $6409 = $55[1]`)在**其执行帧**内读取,注入时刻的
    /// 当前帧未必是同一帧(实测踩坑)。广播写入:全局 + 全部活动帧,
    /// 保证任一层级的键链本帧都能读到。
    pub fn inject_input_key(&mut self, key: &[u8]) -> Result<()> {
        let r = yuris_value::VarRef {
            space: yuris_value::VarSpace::Dollar,
            id: 55,
        };
        let v = Value::Str(key.to_vec());
        for f in &mut self.frames {
            if f.locals.has_array(&r) {
                let _ = f.locals.set_elem(&r, &[1], v.clone());
            }
        }
        self.store.set_elem(&r, &[1], v)
    }

    /// 诊断:读注入串(顶帧优先,与求值读路径对称)。
    pub fn input_key_snapshot(&self) -> String {
        let r = yuris_value::VarRef {
            space: yuris_value::VarSpace::Dollar,
            id: 55,
        };
        if let Some(f) = self.frames.last() {
            if let Ok(yuris_value::Value::Str(bs)) = f.locals.get_elem(&r, &[1]) {
                return String::from_utf8_lossy(bs).into_owned();
            }
        }
        match self.store.get_elem(&r, &[1]) {
            Ok(yuris_value::Value::Str(bs)) => String::from_utf8_lossy(bs).into_owned(),
            _ => "<undef>".into(),
        }
    }

    /// 清除输入注入(全帧 + 全局;见 [`Self::inject_input_key`])。
    pub fn clear_input_key(&mut self) -> Result<()> {
        let r = yuris_value::VarRef {
            space: yuris_value::VarSpace::Dollar,
            id: 55,
        };
        let v = Value::Str(Vec::new());
        for f in &mut self.frames {
            if f.locals.has_array(&r) {
                let _ = f.locals.set_elem(&r, &[1], v.clone());
            }
        }
        self.store.set_elem(&r, &[1], v)
    }
}
