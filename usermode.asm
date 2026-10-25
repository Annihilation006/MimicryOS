; ============================================================
; usermode.asm — Ring 3 用户态全家桶, 纯汇编无 C 无 Rust
; GDT/TSS 自建, 页表 U/S 修补, int 0x80 门, 微点阵字库, 用户载荷
; ABI 按设计文档 07: rax=号 rdi=能力令牌 rsi/rdx/r8=参数
;   1 = putpixel  rsi=x rdx=y r8=color
;   2 = glyph     rsi=ascii rdx=x r8=y r9=color
; ============================================================

[bits 64]

extern idt_set_gate

section .data
align 16
gdt:
    dq 0                          ; null
    dq 0x00AF9A000000FFFF         ; 0x08 kcode L=1
    dq 0x00CF92000000FFFF         ; 0x10 kdata
    dq 0x0020FA000000FFFF         ; 0x18 ucode ring3 L=1
    dq 0x00CFF2000000FFFF         ; 0x20 udata ring3
    dq 0                          ; 0x28 tss 低 8B (运行时填)
    dq 0                          ;       tss 高 8B
gdtr:
    dw 63
    dq gdt

palette dd 0x00FFFF, 0xFF00FF, 0xFFFFFF, 0x00FF00
msg     db "RING3 OK", 0

; 3x5 微点阵: ascii + 5 行位图 (bit0=最左), 线性查找够用
microfont:
    db 'R', 7,5,7,6,5
    db 'I', 7,2,2,2,7
    db 'N', 5,7,7,5,5
    db 'G', 7,4,5,5,7
    db '3', 7,1,7,1,7
    db ' ', 0,0,0,0,0
    db 'O', 7,5,5,5,7
    db 'K', 5,6,4,6,5
    db 0

section .bss
align 16
tss:        resb 128
irq_stack0: resq 512            ; ring3 进来的内核栈 (TSS RSP0)
user_stack: resq 512            ; 用户栈
fb_base:    resq 1
fb_scan:    resq 1

section .text

; ---------------- 初始化: lgdt/ltr + 页表放行 + 开 0x80 门 ----------------
global usermode_init
usermode_init:
    ; TSS 描述符 (16B) 手搓
    mov rax, tss
    mov rbx, 103
    mov rcx, rax
    and rcx, 0xFFFF
    shl rcx, 16
    or rbx, rcx                 ; limit15_0 | base15_0<<16
    mov rcx, rax
    shr rcx, 16
    and rcx, 0xFF
    shl rcx, 32
    or rbx, rcx                 ; base23_16<<32
    mov rcx, 0x89
    shl rcx, 40
    or rbx, rcx                 ; type=0x89 available 64bit TSS, P=1
    mov rcx, 103
    shr rcx, 16
    and rcx, 0xF
    shl rcx, 48
    or rbx, rcx                 ; limit19_16<<48
    mov rcx, rax
    shr rcx, 24
    and rcx, 0xFF
    shl rcx, 56
    or rbx, rcx                 ; base31_24<<56
    mov [gdt + 5*8], rbx
    mov rbx, rax
    shr rbx, 32
    and rbx, 0xFFFFFFFF
    mov [gdt + 6*8], rbx        ; base63_32

    lgdt [gdtr]
    push qword 0x08
    lea rax, [rel .flush]
    push rax
    retfq
.flush:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov ax, 0x0
    mov fs, ax
    mov gs, ax

    mov ax, 0x28
    ltr ax

    ; RSP0 = irq_stack0 顶 (TSS 偏移 4)
    lea rax, [irq_stack0 + 4096]
    mov [tss + 4], rax

    ; fb 基址/行距从 bootboot 结构抄 (fb_ptr@0x38, fb_scanline@0x4C)
    mov rax, [0xffffffffffe00000 + 0x28]
    mov [fb_base], rax
    mov eax, [0xffffffffffe00000 + 0x3C]
    mov [fb_scan], rax

    ; 页表放行: 用户代码区 + 用户栈区, 两段各走一遍四级页表
    mov rcx, user_payload_start
    call patch_user_region
    mov rcx, user_stack
    call patch_user_region

    ; IDT 128 号门: DPL=3 让 ring3 能 int
    mov edi, 128
    mov rsi, syscall_int_entry
    mov edx, 0x08
    mov ecx, 0xEE
    call idt_set_gate
    ret

; rcx = va, 沿 PML4→PDPT→PD→PT 全路 U/S 置位 + NX 清位, 1G/2M/4K 页都认
patch_user_region:
    mov rax, cr3
    and rax, 0x000FFFFFFFFFF000
    ; PML4
    mov r8, rcx
    shr r8, 39
    and r8, 511
    mov rdx, [rax + r8*8]
    or rdx, 4
    btr rdx, 63
    mov [rax + r8*8], rdx
    mov r10, rdx
    and r10, 0x000FFFFFFFFFF000     ; pdpt 表
    ; PDPT
    mov r9, rcx
    shr r9, 30
    and r9, 511
    mov rdx, [r10 + r9*8]
    test rdx, 0x80
    jz .pdpt_not_leaf
    or rdx, 4
    btr rdx, 63
    mov [r10 + r9*8], rdx
    jmp .flush_tlb
.pdpt_not_leaf:
    or rdx, 4
    btr rdx, 63
    mov [r10 + r9*8], rdx
    mov r10, rdx
    and r10, 0x000FFFFFFFFFF000     ; pd 表
    ; PD
    mov r11, rcx
    shr r11, 21
    and r11, 511
    mov rdx, [r10 + r11*8]
    test rdx, 0x80
    jz .pd_not_leaf
    or rdx, 4
    btr rdx, 63
    mov [r10 + r11*8], rdx
    jmp .flush_tlb
.pd_not_leaf:
    or rdx, 4
    btr rdx, 63
    mov [r10 + r11*8], rdx
    mov r10, rdx
    and r10, 0x000FFFFFFFFFF000     ; pt 表 — 512 个 PTE 全放行, 免得 .data 掉别的 4K 页
    xor r11, r11
.pte_loop:
    mov rdx, [r10 + r11*8]
    test rdx, 1
    jz .pte_next
    or rdx, 4
    btr rdx, 63
    mov [r10 + r11*8], rdx
.pte_next:
    inc r11
    cmp r11, 512
    jl .pte_loop
.flush_tlb:
    mov rax, cr3
    mov cr3, rax
    ret

; ---------------- int 0x80 入口 (CPU 已从 RSP0 起栈) ----------------
global syscall_int_entry
syscall_int_entry:
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    cmp rax, 1
    je sys_putpixel
    cmp rax, 2
    je sys_glyph
    jmp sys_done

sys_putpixel:                       ; rsi=x rdx=y r8=color
    mov edi, esi
    mov esi, edx
    mov rdx, r8
    call px_put
    jmp sys_done

sys_glyph:                          ; rsi=ascii rdx=x r8=y r9=color
    mov rbp, microfont
.gfind:
    movzx eax, byte [rbp]
    test eax, eax
    jz sys_done
    cmp eax, esi
    je .gfound
    add rbp, 6
    jmp .gfind
.gfound:
    mov r12, rdx                    ; x
    mov r13, r8                     ; y
    mov r14, r9                     ; color
    xor ebx, ebx                    ; row
.grow:
    movzx eax, byte [rbp + 1 + rbx]
    xor ecx, ecx                    ; col
.gcol:
    mov r11d, eax
    and r11d, 1
    jz .gnext
    xor r8d, r8d                    ; dy
.gdy:
    xor r10d, r10d                  ; dx
.gdx:
    mov edi, ecx
    imul edi, 3
    add edi, r12d
    add edi, r10d
    mov esi, ebx
    imul esi, 3
    add esi, r13d
    add esi, r8d
    mov rdx, r14
    call px_put                     ; 打烂 rax rdx r9 r11, 循环状态都在别的寄存器
    inc r10d
    cmp r10d, 3
    jl .gdx
    inc r8d
    cmp r8d, 3
    jl .gdy
.gnext:
    shr eax, 1
    inc ecx
    cmp ecx, 3
    jl .gcol
    inc ebx
    cmp ebx, 5
    jl .grow
    jmp sys_done

sys_done:
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax
    iretq

; edi=x esi=y rdx=color → fb 像素
px_put:
    mov r9, rdx
    mov rax, rsi
    mul qword [fb_scan]
    mov r11, rax
    mov rax, rdi
    shl rax, 2
    add r11, rax
    add r11, [fb_base]
    mov [r11], r9d
    ret

; ---------------- 用户载荷 (ring3) ----------------
global user_payload_start
user_payload_start:
    mov dx, 0x23
    mov ds, dx
    mov es, dx
    xor r15d, r15d                  ; 相位 → 配色
.phase:
    ; 擦旧下划线
    mov ebx, 27
.erase:
    mov rax, 1
    mov rdi, 1                      ; 能力令牌, 装个样子
    mov rsi, rbx
    mov rdx, 180
    xor r8d, r8d
    int 0x80
    inc ebx
    cmp ebx, 100
    jl .erase
    ; 新下划线, 长度跟相位走
    mov eax, r15d
    xor edx, edx
    mov ecx, 60
    div ecx
    add edx, 8
    mov ebx, 27
.uline:
    cmp ebx, edx
    jae .text
    mov rax, 1
    mov rdi, 1
    mov rsi, rbx
    mov rdx, 180
    mov r8d, 0xFFFFFF
    int 0x80
    inc ebx
    jmp .uline
.text:
    lea rbx, [msg]
    xor r12, r12
.tloop:
    movzx esi, byte [rbx + r12]
    test esi, esi
    jz .tdone
    mov rax, 2
    mov rdi, 1
    mov rdx, r12
    imul rdx, rdx, 12
    add rdx, 27
    mov r8d, 160
    lea r9, [palette]
    mov r9d, [r9 + r15*4]
    int 0x80
    inc r12
    jmp .tloop
.tdone:
    inc r15d
    and r15d, 3
    mov rcx, 2000000
.delay:
    dec rcx
    jnz .delay
    jmp .phase

global user_stack_top
user_stack_top equ user_stack + 4096
