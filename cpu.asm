; ============================================================
; MimicryOS test 分支 · x86_64 汇编地基（纯 CPU 原语）
; 维护：homo-ai (Hugo_YuGuo)
;
; 定位与边界（详见 TEST-BRANCH.md）：
;   - 本文件只提供最底层的 CPU 访问原语，供后续 Rust 代码
;     经 extern "C" 调用，是分页 / 内存管理 / 定时器的地基。
;   - 刻意不涉及 IDT、中断入口、系统调用与上下文切换——
;     这些属于 main 分支正在进行的方向，避免双线实现。
;   - 所有导出符号统一加 asm_ 前缀，不与既有 C 代码符号重名。
;
; 调用约定：System V AMD64（整数参数 rdi/rsi/rdx，返回 rax）。
; ============================================================

[bits 64]
default rel

section .text

; ---------------- 端口 I/O ----------------
; PS/2、ATA(PIO)、传统 PIT 等设备都靠端口访问，
; 统一收口到这里，Rust 侧不再内嵌汇编。

global asm_inb
asm_inb:                        ; unsigned char asm_inb(unsigned short port)
    xor eax, eax
    mov dx, di
    in al, dx
    ret

global asm_inw
asm_inw:                        ; unsigned short asm_inw(unsigned short port)
    xor eax, eax
    mov dx, di
    in ax, dx
    ret

global asm_outb
asm_outb:                       ; void asm_outb(unsigned short port, unsigned char val)
    mov dx, di
    mov al, sil
    out dx, al
    ret

global asm_outw
asm_outw:                       ; void asm_outw(unsigned short port, unsigned short val)
    mov dx, di
    mov ax, si
    out dx, ax
    ret

global asm_io_wait
asm_io_wait:                    ; void asm_io_wait(void)
    mov dx, 0x80                ; 写 POST 调试端口制造约 1us 延时（ISA 时代惯例）
    out dx, al
    ret

; ---------------- 中断开关 / 停机 ----------------

global asm_cli
asm_cli:                        ; void asm_cli(void)
    cli
    ret

global asm_sti
asm_sti:                        ; void asm_sti(void)
    sti
    ret

global asm_hlt
asm_hlt:                        ; void asm_hlt(void)
    hlt
    ret

global asm_halt_forever
asm_halt_forever:               ; void asm_halt_forever(void)  关中断并永久等待
    cli
.spin:
    hlt
    jmp .spin

; ---------------- 控制寄存器 ----------------
; CR0/CR4 的各标志位、CR3 页表基址是后续启用分页的入口，
; CR2 记录最近一次缺页的线性地址。

global asm_read_cr0
asm_read_cr0:                   ; unsigned long long asm_read_cr0(void)
    mov rax, cr0
    ret

global asm_read_cr2
asm_read_cr2:                   ; unsigned long long asm_read_cr2(void)
    mov rax, cr2
    ret

global asm_read_cr3
asm_read_cr3:                   ; unsigned long long asm_read_cr3(void)
    mov rax, cr3
    ret

global asm_read_cr4
asm_read_cr4:                   ; unsigned long long asm_read_cr4(void)
    mov rax, cr4
    ret

global asm_write_cr3
asm_write_cr3:                  ; void asm_write_cr3(unsigned long long val)
    mov cr3, rdi
    ret

; ---------------- CPUID ----------------
; void asm_cpuid(unsigned int leaf, unsigned int subleaf, unsigned int out[4])
; out[0..3] 依次为 EAX/EBX/ECX/EDX。rbx 为被调用者保存寄存器，需保护。
global asm_cpuid
asm_cpuid:
    push rbx
    mov eax, edi
    mov ecx, esi
    cpuid
    mov [rdx], eax
    mov [rdx + 4], ebx
    mov [rdx + 8], ecx
    mov [rdx + 12], edx
    pop rbx
    ret

; ---------------- 时间戳计数器 ----------------
global asm_rdtsc
asm_rdtsc:                      ; unsigned long long asm_rdtsc(void)
    rdtsc                       ; 结果在 EDX:EAX，拼成 64 位返回
    shl rdx, 32
    or rax, rdx
    ret

; ---------------- 页表维护 ----------------
; unmap 后必须失效 TLB 项，否则旧映射在部分 CPU 上仍可见。

global asm_invlpg
asm_invlpg:                     ; void asm_invlpg(unsigned long long addr)
    invlpg [rdi]
    ret

; ---------------- 模型特定寄存器 MSR ----------------
; EFER(APIC/长模式控制)、LAPIC 基址等都住在 MSR 里，
; 启用 APIC 定时器前必须先能读写它。

global asm_rdmsr
asm_rdmsr:                      ; unsigned long long asm_rdmsr(unsigned int msr)
    mov rcx, rdi
    rdmsr
    shl rdx, 32
    or rax, rdx
    ret

global asm_wrmsr
asm_wrmsr:                      ; void asm_wrmsr(unsigned int msr, unsigned long long val)
    mov rcx, rdi
    mov rax, rsi
    mov rdx, rsi
    shr rdx, 32
    wrmsr
    ret
