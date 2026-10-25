/* ============================================================
 * cpu.h —— x86_64 汇编地基的 C 侧声明（实现见 cpu.asm）
 * 维护：homo-ai (Hugo_YuGuo)
 *
 * 设计说明：
 *   - 所有符号加 asm_ 前缀，避免与既有代码重名；
 *   - 类型直接使用 C 内建类型，不依赖 kernel.c 里的 typedef，
 *     任何翻译单元都能安全包含本头文件；
 *   - 后续 Rust 代码通过 extern "C" 声明同名函数即可调用。
 * ============================================================ */
#ifndef CPU_H
#define CPU_H

#ifdef __cplusplus
extern "C" {
#endif

/* ---- 端口 I/O ---- */
unsigned char  asm_inb(unsigned short port);
unsigned short asm_inw(unsigned short port);
void asm_outb(unsigned short port, unsigned char val);
void asm_outw(unsigned short port, unsigned short val);
void asm_io_wait(void);

/* ---- 中断开关 / 停机 ---- */
void asm_cli(void);
void asm_sti(void);
void asm_hlt(void);
void asm_halt_forever(void);

/* ---- 控制寄存器 ---- */
unsigned long long asm_read_cr0(void);
unsigned long long asm_read_cr2(void);   /* 最近一次缺页的线性地址 */
unsigned long long asm_read_cr3(void);   /* 当前页表物理基址 */
unsigned long long asm_read_cr4(void);
void asm_write_cr3(unsigned long long val);
void asm_invlpg(unsigned long long addr);   /* 失效单页 TLB 项 */

/* ---- CPUID ----
 * out[0..3] 依次为 EAX/EBX/ECX/EDX。
 * 例：leaf=1 时 EDX 位 5 = 支持 RDTSC，EDX 位 6 = 支持 PAE。 */
void asm_cpuid(unsigned int leaf, unsigned int subleaf, unsigned int out[4]);

/* ---- 时间戳计数器 ---- */
unsigned long long asm_rdtsc(void);

/* ---- 模型特定寄存器 ----
 * 常用编号：EFER=0xC0000080（长模式/SYSRET 控制），
 * LAPIC_BASE=0x1B（本地 APIC 基址寄存器）。 */
unsigned long long asm_rdmsr(unsigned int msr);
void asm_wrmsr(unsigned int msr, unsigned long long val);

#ifdef __cplusplus
}
#endif

#endif /* CPU_H */
