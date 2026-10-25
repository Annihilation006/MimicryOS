[BITS 64]
global switch_to_task

switch_to_task:
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15
    
    mov [rdi], rsp
    mov rsp, rsi
    
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp
    
    ret
