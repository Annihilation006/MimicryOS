; ============================================================
; vmo.asm — VMO 共享内存系统调用, 纯汇编
; VMO = 用户态共享内存对象: create 从池里切 16KB, 返回基址(恒等可写)
; 池在内核 .bss 顶部 2MB 窗口内, 已被 patch_user_region 放行为用户可读写
; 系统调用: 9 = create(rdi=npages, 暂定固定16K) → rax=基址
;          10 = getaddr(rdi=槽号) → rax=基址 (给第二个用户任务用)
; ============================================================

[bits 64]

section .bss
align 16
global vmo_table
vmo_table: resq 24            ; 8 槽 × (基址, 魔数) — 魔数非零 = 在用
vmo_pool:  resb 131072        ; 8 × 16KB 共享区
vmo_bump:  resq 1

section .text

global vmo_init
vmo_init:
    xor eax, eax
    mov rcx, 16
    mov rdi, vmo_table
.clr:
    mov [rdi], rax
    add rdi, 8
    dec rcx
    jnz .clr
    mov rax, vmo_pool
    mov [vmo_bump], rax
    ret

; rdi = 帧指针 (同 SyscallFrame 布局: rax@112, rdi_arg@72)
; 返回 rax = 新 rsp (0 = 不切换, vmo 不切换)
global vmo_syscall
vmo_syscall:
    mov rax, [rdi + 112]        ; 系统调用号
    cmp rax, 9
    je .create
    cmp rax, 10
    je .getaddr
    mov qword [rdi + 112], -1   ; 未知号
    xor eax, eax
    ret

.create:
    ; 找空槽: 魔数 == 0 (槽距 24B = lea ×3 再 ×8, NASM 不认 *24)
    xor rcx, rcx
.find:
    cmp rcx, 8
    jae .fail
    lea rdx, [rcx + rcx*2]
    shl rdx, 3
    cmp qword [vmo_table + rdx + 8], 0
    je .got
    inc rcx
    jmp .find
.got:
    ; 池里切 16KB
    mov rax, [vmo_bump]
    add qword [vmo_bump], 16384
    mov r8, vmo_pool + 131072
    cmp rax, r8
    ja .fail                    ; 池满
    mov [vmo_table + rdx], rax          ; 基址
    mov qword [vmo_table + rdx + 8], 0x564D4F21  ; 'VMO!' 魔数
    mov [rdi + 112], rax        ; 返回基址
    xor eax, eax
    ret
.fail:
    mov qword [rdi + 112], -1
    xor eax, eax
    ret

.getaddr:
    mov rsi, [rdi + 72]         ; 槽号
    cmp rsi, 8
    jae .fail
    lea rdx, [rsi + rsi*2]
    shl rdx, 3
    cmp qword [vmo_table + rdx + 8], 0
    je .fail
    mov rax, [vmo_table + rdx]
    mov [rdi + 112], rax
    xor eax, eax
    ret
