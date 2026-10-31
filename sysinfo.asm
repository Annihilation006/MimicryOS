; ============================================================
; sysinfo.asm — CPU 信息收集器, 纯汇编
; 一次调用遍历 CPUID 叶子, 结果填进调用方给的结构体:
;   0x00  vendor[12]      叶 0 的 EBX/EDX/ECX ("GenuineIntel" 等)
;   0x0C  brand[48]       叶 0x80000002..4 的 48 字节型号串
;   0x3C  signature       叶 1 EAX (stepping/model/family 打包)
;   0x40  feat_ecx        叶 1 ECX 特性位
;   0x44  feat_edx        叶 1 EDX 特性位
;   0x48  addr_bits       叶 0x80000008 EAX (物理/线性地址位宽)
;   0x4C  max_leaf        叶 0 EAX
;   0x50  max_ext_leaf    叶 0x80000000 EAX
;   0x54  invariant_tsc   叶 0x80000007 EDX bit8 (TSC 恒定率)
;   0x58  has_tsc         叶 1 EDX bit4
; 结构体 0x60 = 96 字节, 由调用方分配。
; 注意: 内核链在高地址 (--no-pie), 本文件无静态数据, 全寄存器操作。
; ============================================================

[bits 64]

section .text

global asm_sysinfo_collect

; void asm_sysinfo_collect(void *out)
; rdi = out 结构体指针 (>= 96 字节)
asm_sysinfo_collect:
    push rbx
    push r12
    mov r8, rdi                 ; r8 = out 基址, 全程不动

    ; ---- 叶 0: 厂商串 + 最大叶子 ----
    xor eax, eax
    cpuid
    mov [r8 + 0x4C], eax        ; max_leaf
    mov [r8 + 0x00], ebx        ; "Genu"
    mov [r8 + 0x04], edx        ; "ineI"
    mov [r8 + 0x08], ecx        ; "ntel"

    ; ---- 叶 1: 签名 + 特性位 ----
    mov eax, 1
    cpuid
    mov [r8 + 0x3C], eax        ; signature
    mov [r8 + 0x40], ecx        ; feat_ecx
    mov [r8 + 0x44], edx        ; feat_edx

    ; ---- TSC 存在位: 叶 1 EDX bit4 ----
    mov eax, edx
    and eax, 0x10
    shr eax, 4
    mov [r8 + 0x58], eax        ; has_tsc

    ; ---- 扩展叶子: 先探上限, 够了才读 ----
    mov eax, 0x80000000
    cpuid
    mov [r8 + 0x50], eax        ; max_ext_leaf
    mov r9d, eax

    ; ---- 型号串: 叶 0x80000002..4, 每次 16 字节 ----
    cmp r9d, 0x80000004
    jb .no_brand
    lea r10, [r8 + 0x0C]        ; brand 区
    mov r11d, 0x80000002        ; r11 = 当前叶子
.brand_loop:
    mov eax, r11d
    cpuid
    mov [r10], eax
    mov [r10 + 4], ebx
    mov [r10 + 8], ecx
    mov [r10 + 12], edx
    add r10, 16
    inc r11d
    cmp r11d, 0x80000005
    jb .brand_loop
    jmp .addr_bits
.no_brand:
    ; 不支持也把 48 字节清零, 结构体不留脏数据
    xor eax, eax
    lea r10, [r8 + 0x0C]
    mov dword [r10], eax
    mov dword [r10 + 4], eax
    mov dword [r10 + 44], eax
    mov qword [r10 + 8], rax
    mov qword [r10 + 16], rax
    mov qword [r10 + 24], rax
    mov qword [r10 + 32], rax
    mov qword [r10 + 40], rax

.addr_bits:
    ; ---- 叶 0x80000008: 地址位宽 ----
    cmp r9d, 0x80000008
    jb .no_addr
    mov eax, 0x80000008
    cpuid
    mov [r8 + 0x48], eax
    jmp .inv_tsc
.no_addr:
    mov dword [r8 + 0x48], 0

.inv_tsc:
    ; ---- 叶 0x80000007: 不变率 TSC 旗标 (EDX bit8) ----
    cmp r9d, 0x80000007
    jb .no_inv
    mov eax, 0x80000007
    cpuid
    mov eax, edx
    and eax, 0x100
    shr eax, 8
    mov [r8 + 0x54], eax
    jmp .done
.no_inv:
    mov dword [r8 + 0x54], 0

.done:
    pop r12
    pop rbx
    ret
