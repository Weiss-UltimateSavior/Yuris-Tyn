// P1 引擎真值 hook 的前置侦察(GhidraScript Java 版):
//   1) 反编译 hook 设计所需的关键函数(主循环/任务轮转/加载器/GO/表初始化/启动链)
//   2) 扫 FUN_0046305c 的 MOV 指令提取 121 项处理器表(槽位->地址)
// 输出: docs/reverse/decompiled/engine/hook_recon/ 下
//   recon_<addr>_<NAME>.c + handler_table.txt
//
// @category Yuris
import java.io.File;
import java.io.FileOutputStream;
import java.io.PrintStream;
import java.util.TreeMap;

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.InstructionIterator;
import ghidra.program.model.scalar.Scalar;

public class HookRecon extends GhidraScript {

    static final String OUTDIR = "D:\\yuris-kernel\\docs\\reverse\\decompiled\\engine\\hook_recon";
    static final long TABLE_BASE = 0x0078B020L;
    static final long INIT_FUNC = 0x0046305CL;

    static final long[][] TARGETS = {
        { 0x0040449cL, 0 }, // main_loop
        { 0x0043be6cL, 1 }, // task_rotate
        { 0x00450dfdL, 2 }, // loader
        { 0x00451348L, 3 }, // ysvr_apply
        { 0x0046305cL, 4 }, // table_init
        { 0x0044272cL, 5 }, // GO
        { 0x004428c0L, 6 }, // GOSUB
        { 0x0044b418L, 7 }, // RETURN
        { 0x004431ecL, 8 }, // IF
        { 0x00451724L, 9 }, // script_end
        { 0x0046b63cL, 10 }, // boot_chain
        { 0x00463c7cL, 11 }, // label_load
        { 0x0045124cL, 12 }, // label_hash_find
    };

    static final String[] TAGS = {
        "main_loop", "task_rotate", "loader", "ysvr_apply", "table_init",
        "GO", "GOSUB", "RETURN", "IF", "script_end", "boot_chain",
        "label_load", "label_hash_find",
    };

    void write(String path, byte[] data) throws Exception {
        FileOutputStream fos = new FileOutputStream(path);
        fos.write(data);
        fos.close();
    }

    @Override
    public void run() throws Exception {
        File dir = new File(OUTDIR);
        if (!dir.exists()) {
            dir.mkdirs();
        }
        DecompInterface di = new DecompInterface();
        di.openProgram(currentProgram);

        for (long[] t : TARGETS) {
            Address addr = currentProgram.getAddressFactory().getDefaultAddressSpace()
                    .getAddress(t[0]);
            Function f = getFunctionManager().getFunctionAt(addr);
            if (f == null) {
                println("NO FUNC at " + Long.toHexString(t[0]) + " " + TAGS[t[1]]);
                continue;
            }
            DecompileResults res = di.decompileFunction(f, 120, monitor);
            if (!res.decompileCompleted()) {
                println("DECOMP FAIL " + Long.toHexString(t[0]) + " " + TAGS[t[1]]);
                continue;
            }
            byte[] c = res.getDecompiledFunction().getC().getBytes("UTF-8");
            write(OUTDIR + "\\recon_" + String.format("%08x", t[0]) + "_" + TAGS[t[1]] + ".c", c);
            println("OK " + Long.toHexString(t[0]) + " " + TAGS[t[1]] + " (" + c.length + " bytes)");
        }

        // ---- 2. 扫表初始化函数的 MOV 提取处理器表 ----
        Address initAddr = currentProgram.getAddressFactory().getDefaultAddressSpace()
                .getAddress(INIT_FUNC);
        Function f = getFunctionManager().getFunctionAt(initAddr);
        TreeMap<Long, Long> assigns = new TreeMap<>();
        if (f == null) {
            println("FAIL: no function at " + Long.toHexString(INIT_FUNC));
        } else {
            InstructionIterator it = getListing().getInstructions(f.getBody(), true);
            while (it.hasNext()) {
                Instruction ins = it.next();
                if (!"MOV".equalsIgnoreCase(ins.getMnemonicString())) {
                    continue;
                }
                // 目标操作数 0: [disp] 形式
                if (ins.getNumOperands() < 1) {
                    continue;
                }
                long disp = -1;
                for (int oi = 0; oi < ins.getNumOperands(); oi++) {
                    for (Object o : ins.getOpObjects(oi)) {
                        if (o instanceof Scalar) {
                            long v = ((Scalar) o).getUnsignedValue();
                            if (v >= TABLE_BASE && v < TABLE_BASE + 0x400) {
                                disp = v;
                            }
                        }
                    }
                }
                if (disp < 0) {
                    continue;
                }
                // 源立即数 = 处理器地址
                for (Object o : ins.getOpObjects(1)) {
                    if (o instanceof Scalar) {
                        long v = ((Scalar) o).getUnsignedValue() & 0xFFFFFFFFL;
                        if (v >= 0x00400000L && v < 0x00500000L) {
                            assigns.put(disp, v);
                        }
                    }
                }
            }
        }
        StringBuilder sb = new StringBuilder();
        sb.append(String.format("# DAT_0078b020 处理器表(扫 %08x MOV 提取, %d 项)%n",
                INIT_FUNC, assigns.size()));
        sb.append("# 格式: index cmd 表偏移 处理器地址%n%n");
        for (java.util.Map.Entry<Long, Long> e : assigns.entrySet()) {
            long idx = (e.getKey() - TABLE_BASE) / 4;
            long cmd = idx - 8;
            sb.append(String.format("%4d  cmd=%4d(0x%02x)  [%08x]  -> %08x%n",
                    idx, cmd, cmd & 0xFF, e.getKey(), e.getValue()));
        }
        write(OUTDIR + "\\handler_table.txt", sb.toString().getBytes("UTF-8"));
        println("TABLE: " + assigns.size() + " entries");
        println("DONE -> " + OUTDIR);
    }
}
