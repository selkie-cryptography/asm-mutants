//! Instruction-set knowledge: which mnemonics swap, which operands commute,
//! and how a flag-dependent instruction is made to see the flag clear or
//! set.

mod aarch64;

pub use aarch64::Aarch64;

use crate::{instruction::Instruction, operators::Mutation};

/// What the operators need to know about one instruction set.
pub trait Architecture {
    /// Mnemonic swaps. Both directions are listed where both encode.
    fn mnemonic_swaps(&self) -> &'static [(&'static str, &'static str)];

    /// Ops whose last two operands do not commute.
    fn is_noncommutative(&self, mnemonic: &str) -> bool;

    /// Register-pair loads and stores: swapping the pair is a mutant.
    fn is_pair(&self, mnemonic: &str) -> bool;

    /// Memory ops: offsets step by a vector register width so the encoding
    /// stays valid.
    fn is_load_store(&self, mnemonic: &str) -> bool;

    /// The instruction with the carry forced clear.
    fn carry_clear(&self, instruction: &Instruction) -> Option<Mutation>;

    /// The instruction with the carry forced set.
    fn carry_set(&self, instruction: &Instruction) -> Option<Mutation>;

    /// A conditional select pinned to each of its inputs.
    fn select_mutations(&self, instruction: &Instruction) -> Vec<Mutation>;
}
