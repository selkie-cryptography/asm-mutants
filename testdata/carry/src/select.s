// min(x0, x1), unreferenced: a GAS site for the listing tests.
.text
.globl _asm_mutants_select_min
_asm_mutants_select_min:
    cmp x0, x1
    csel x0, x0, x1, lo
    ret
