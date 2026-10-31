; uapp/argv.asm — 参数版 ring3 用户程序 (NASM -f bin 裸二进制)
;
; 装载 ABI:
;   代码经 120 字节 ELF 包装头落在 base+120; 内核 spawn 时把参数块
;   写进栈页底部: base+0x1000 = 参数长度, base+0x1008 = 参数指针。
;
; 自定位: call/pop 取到运行期地址 (base+HDR+5), 参数页偏移用常量
; 表达, 不需要任何魔数位移 —— NASM 直接算 [rbp+常量]。
;
; 行为: write(3, argv, len) 把参数串画到行 20, 然后 exit(0)。

BITS 64

ARG equ 0x1000                ; 参数块所在页内偏移
HDR equ 120                   ; ELF 包装头大小

start:
    call x                      ; 压入 x 的运行地址
x:
    pop rbp                     ; rbp = base + HDR + 5
    mov rax, [rbp + ARG-HDR-5]  ; 参数长度   (base+0x1000)
    mov rdx, [rbp + ARG-HDR+3]  ; 参数指针   (base+0x1008)
    mov rsi, rdx
    mov rdx, rax
    mov eax, 1                  ; SYS_WRITE
    mov edi, 3                  ; fd 3 → 行 20
    int 0x80
    mov eax, 60                 ; SYS_EXIT
    int 0x80
