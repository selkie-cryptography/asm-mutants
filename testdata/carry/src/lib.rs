//! Integration-test fixture: a 64-bit add with carry out, in inline
//! assembly on aarch64, and a `global_asm!` select that nothing calls.
//!
//! The one test covers the no-carry case only, so the `adc` carry-clear
//! mutant survives and the carry-set mutant is caught.

#![allow(clippy::missing_safety_doc)]

#[cfg(target_arch = "aarch64")]
core::arch::global_asm!(include_str!("select.s"));

/// `a + b`, as `(sum, carry)`.
#[cfg(target_arch = "aarch64")]
pub fn add_carry(a: u64, b: u64) -> (u64, u64) {
    let sum: u64;
    let carry: u64;
    // SAFETY: register-only arithmetic, no memory.
    unsafe {
        core::arch::asm!(
            "adds {sum}, {a}, {b}",
            "adc {carry}, xzr, xzr",
            a = in(reg) a,
            b = in(reg) b,
            sum = out(reg) sum,
            carry = out(reg) carry,
            options(pure, nomem, nostack),
        );
    }
    (sum, carry)
}

/// `a + b`, as `(sum, carry)`.
#[cfg(not(target_arch = "aarch64"))]
pub fn add_carry(a: u64, b: u64) -> (u64, u64) {
    let (sum, carry) = a.overflowing_add(b);
    (sum, u64::from(carry))
}

#[cfg(test)]
mod tests {
    use super::add_carry;

    #[test]
    fn small_sum() {
        assert_eq!(add_carry(1, 2), (3, 0));
    }
}
