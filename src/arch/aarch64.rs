//! AArch64, scalar and NEON.
//!
//! The carry operators follow <https://words.filippo.io/assembly-mutation/>:
//! a flag-setting form gets a flag-writing prefix, since it resets the flags
//! anyway; a plain form gets a `+-1` suffix instead, which leaves the flags
//! alone.

use super::Architecture;
use crate::{
    instruction::Instruction,
    operators::{Mutation, Operator},
};

const MNEMONIC_SWAPS: &[(&str, &str)] = &[
    ("add", "sub"),
    ("sub", "add"),
    ("adds", "subs"),
    ("subs", "adds"),
    ("mla", "mls"),
    ("mls", "mla"),
    ("mul", "mla"),
    ("sqrdmulh", "sqdmulh"),
    ("sqdmulh", "sqrdmulh"),
    ("and", "orr"),
    ("orr", "and"),
    ("eor", "orr"),
    ("bic", "and"),
    ("orn", "orr"),
    ("eon", "eor"),
    ("shl", "ushr"),
    ("ushr", "shl"),
    ("sshr", "shl"),
    ("lsl", "lsr"),
    ("lsr", "lsl"),
    ("ror", "lsr"),
    ("zip1", "zip2"),
    ("zip2", "zip1"),
    ("uzp1", "uzp2"),
    ("uzp2", "uzp1"),
    ("trn1", "trn2"),
    ("trn2", "trn1"),
    ("cbnz", "cbz"),
    ("cbz", "cbnz"),
    ("tbnz", "tbz"),
    ("tbz", "tbnz"),
    ("b.eq", "b.ne"),
    ("b.ne", "b.eq"),
    ("b.lo", "b.hs"),
    ("b.hs", "b.lo"),
    ("b.lt", "b.ge"),
    ("b.ge", "b.lt"),
    ("b.gt", "b.le"),
    ("b.le", "b.gt"),
];

const NONCOMMUTATIVE: &[&str] = &[
    "sub", "subs", "sbc", "sbcs", "bic", "bics", "orn", "eon", "bcax",
];

const PAIRS: &[&str] = &["ldp", "stp", "ldnp", "stnp"];

const LOAD_STORE: &[&str] = &[
    "ldp", "stp", "ldnp", "stnp", "ldr", "str", "ldur", "stur", "ld1", "st1", "ld2", "st2", "ld3",
    "st3", "ld4", "st4",
];

#[derive(Clone, Copy, Debug, Default)]
pub struct Aarch64;

impl Aarch64 {
    /// The zero register matching a general register's width. An `asm!`
    /// placeholder is 64-bit unless it carries the `:w` modifier.
    fn zero_register(register: &str) -> Option<&'static str> {
        if let Some(inner) = register.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
            return Some(if inner.ends_with(":w") { "wzr" } else { "xzr" });
        }
        match register.chars().next()?.to_ascii_lowercase() {
            'x' => Some("xzr"),
            'w' => Some("wzr"),
            _ => None,
        }
    }

    fn is_zero_register(register: &str) -> bool {
        register.eq_ignore_ascii_case("xzr") || register.eq_ignore_ascii_case("wzr")
    }

    /// `subs zr, zr, zr` sets the carry, `adds zr, zr, zr` clears it.
    fn flag_writer(mnemonic: &str, zero: &str) -> Instruction {
        Instruction::new(mnemonic, &[zero, zero, zero])
    }

    /// `add d, d, #1` or `sub d, d, #1`: the carry applied after the fact.
    fn step(mnemonic: &str, destination: &str) -> Instruction {
        Instruction::new(mnemonic, &[destination, destination, "#1"])
    }

    /// `(destination, zero register)` for a three-operand carry instruction.
    fn carry_operands(instruction: &Instruction) -> Option<(&str, &'static str)> {
        if instruction.operands.len() != 3 {
            return None;
        }
        let destination = instruction.operands[0].as_str();
        Some((destination, Self::zero_register(destination)?))
    }
}

impl Architecture for Aarch64 {
    fn mnemonic_swaps(&self) -> &'static [(&'static str, &'static str)] {
        MNEMONIC_SWAPS
    }

    fn is_noncommutative(&self, mnemonic: &str) -> bool {
        NONCOMMUTATIVE.contains(&mnemonic)
    }

    fn is_pair(&self, mnemonic: &str) -> bool {
        PAIRS.contains(&mnemonic)
    }

    fn is_load_store(&self, mnemonic: &str) -> bool {
        LOAD_STORE.contains(&mnemonic)
    }

    fn carry_clear(&self, instruction: &Instruction) -> Option<Mutation> {
        let (destination, zero) = Self::carry_operands(instruction)?;
        let replacement = match instruction.mnemonic.as_str() {
            "adcs" => vec![instruction.with_mnemonic("adds")],
            "sbcs" => vec![Self::flag_writer("adds", zero), instruction.clone()],
            "adc" => vec![instruction.with_mnemonic("add")],
            // Nothing to step into a zero-register destination.
            "sbc" if Self::is_zero_register(destination) => return None,
            "sbc" => vec![
                instruction.with_mnemonic("sub"),
                Self::step("sub", destination),
            ],
            _ => return None,
        };
        Some(Mutation::new(Operator::CarryClear, replacement))
    }

    fn carry_set(&self, instruction: &Instruction) -> Option<Mutation> {
        let (destination, zero) = Self::carry_operands(instruction)?;
        let replacement = match instruction.mnemonic.as_str() {
            "adcs" => vec![Self::flag_writer("subs", zero), instruction.clone()],
            "sbcs" => vec![instruction.with_mnemonic("subs")],
            "adc" if Self::is_zero_register(destination) => return None,
            "adc" => vec![
                instruction.with_mnemonic("add"),
                Self::step("add", destination),
            ],
            "sbc" => vec![instruction.with_mnemonic("sub")],
            _ => return None,
        };
        Some(Mutation::new(Operator::CarrySet, replacement))
    }

    fn select_mutations(&self, instruction: &Instruction) -> Vec<Mutation> {
        if instruction.mnemonic != "csel" || instruction.operands.len() != 4 {
            return Vec::new();
        }
        [1, 2]
            .into_iter()
            .map(|source| {
                let mov = Instruction::new(
                    "mov",
                    &[&instruction.operands[0], &instruction.operands[source]],
                );
                Mutation::new(Operator::Csel, vec![mov])
            })
            .collect()
    }
}
