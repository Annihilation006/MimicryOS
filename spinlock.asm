; ============================================================
; spinlock.asm — 原子锁原语, 纯汇编
; 调度器/多核基础设施的地基: 单核下用于关抢占临界区标记,
; 将来 SMP 上就是标准自旋锁。全部 LOCK 前缀原子指令 + PAUSE 等待。
; 约定: 锁字 0 = 空闲, 1 = 被占。内存序由 x86 TSO 保证,
; unlock 用普通存储即可 (前面的临界区写不会越过它)。
; 注意: 内核链在高地址 (--no-pie), 本文件无静态数据。
; ============================================================

[bits 64]

section .text

global asm_spin_trylock
global asm_spin_lock
global asm_spin_unlock
global asm_atomic_xchg
global asm_atomic_add

; int asm_spin_trylock(unsigned long long *lock)
; 一次尝试: 0→1 成功返回 1, 已被占返回 0。不自旋。
asm_spin_trylock:
    xor eax, eax                ; 期待旧值 0
    mov ecx, 1                  ; 想写入的新值
    cmpxchg [rdi], rcx
    sete al                     ; ZF=1 → 拿到锁
    movzx eax, al
    ret

; void asm_spin_lock(unsigned long long *lock)
; 拿不到就 PAUSE 自旋, 拿到才返回。
asm_spin_lock:
.spin:
    xor eax, eax
    mov ecx, 1
    cmpxchg [rdi], rcx
    jz .got
    pause                       ; 超线程友好的等待提示
    jmp .spin
.got:
    ret

; void asm_spin_unlock(unsigned long long *lock)
asm_spin_unlock:
    mov qword [rdi], 0
    ret

; unsigned long long asm_atomic_xchg(unsigned long long *p, unsigned long long v)
; 交换并返回旧值。
asm_atomic_xchg:
    mov rax, rsi
    xchg [rdi], rax
    ret

; unsigned long long asm_atomic_add(unsigned long long *p, unsigned long long d)
; 原子加, 返回加之前的旧值 (计数器/统计的原子累加基础件)。
asm_atomic_add:
    lock xadd [rdi], rsi
    mov rax, rsi
    ret
