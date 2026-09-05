//! The mutation operators, in two families.
//!
//! - `coverage` (`delete`, `mnemonic`, `swap`, `immediate`): change what an
//!   instruction computes. What a vector kernel needs.
//! - `flags` (`carry-clear`, `carry-set`, `csel`): change what a flag-dependent
//!   instruction sees. What a carry-chain field library needs.

use std::fmt::{self, Display, Formatter};

use anyhow::{Result, bail};
use serde::Serialize;

use crate::{
    arch::Architecture,
    instruction::{Immediate, Instruction},
};

/// One way of changing an instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Operator {
    Delete,
    Mnemonic,
    Swap,
    Immediate,
    CarryClear,
    CarrySet,
    Csel,
}

impl Operator {
    pub const ALL: [Self; 7] = [
        Self::Delete,
        Self::Mnemonic,
        Self::Swap,
        Self::Immediate,
        Self::CarryClear,
        Self::CarrySet,
        Self::Csel,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Delete => "delete",
            Self::Mnemonic => "mnemonic",
            Self::Swap => "swap",
            Self::Immediate => "immediate",
            Self::CarryClear => "carry-clear",
            Self::CarrySet => "carry-set",
            Self::Csel => "csel",
        }
    }

    pub fn family(self) -> &'static str {
        match self {
            Self::Delete | Self::Mnemonic | Self::Swap | Self::Immediate => "coverage",
            Self::CarryClear | Self::CarrySet | Self::Csel => "flags",
        }
    }

    /// Whether `token` names this operator, its family, or a group alias
    /// (`carry` for both carry directions).
    fn matches(self, token: &str) -> bool {
        token == self.name()
            || token == self.family()
            || (token == "carry" && matches!(self, Self::CarryClear | Self::CarrySet))
    }
}

impl Display for Operator {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Which operators a run applies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Operators(Vec<Operator>);

impl Operators {
    pub fn all() -> Self {
        Self(Operator::ALL.to_vec())
    }

    /// Resolves operator, family, and group names; an empty list means all.
    pub fn parse(tokens: &[String]) -> Result<Self> {
        if tokens.is_empty() {
            return Ok(Self::all());
        }
        let mut operators = Vec::new();
        for token in tokens.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
            let matched: Vec<Operator> = Operator::ALL
                .into_iter()
                .filter(|operator| operator.matches(token))
                .collect();
            if matched.is_empty() {
                bail!(
                    "unknown operator `{token}`; operators: {}",
                    Operator::ALL
                        .iter()
                        .map(|o| format!("{o} ({})", o.family()))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            operators.extend(matched);
        }
        Ok(Self(operators))
    }

    pub fn enabled(&self, operator: Operator) -> bool {
        self.0.contains(&operator)
    }

    #[cfg(test)]
    pub fn names(&self) -> Vec<&'static str> {
        self.0.iter().map(|o| o.name()).collect()
    }
}

/// One operator applied to one instruction: the instructions that replace
/// it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mutation {
    pub operator: Operator,
    pub replacement: Vec<Instruction>,
}

impl Mutation {
    pub fn new(operator: Operator, replacement: Vec<Instruction>) -> Self {
        Self {
            operator,
            replacement,
        }
    }

    /// The replacement on one line, `;`-separated.
    pub fn describe(&self) -> String {
        self.replacement
            .iter()
            .map(Instruction::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    }
}

impl Instruction {
    /// Every mutation of this instruction under the given operators.
    pub fn mutations(&self, arch: &dyn Architecture, operators: &Operators) -> Vec<Mutation> {
        let mut out = Vec::new();
        let n = self.operands.len();

        if operators.enabled(Operator::Delete) {
            out.push(Mutation::new(Operator::Delete, vec![Self::nop()]));
        }

        if operators.enabled(Operator::Mnemonic) {
            for (from, to) in arch.mnemonic_swaps() {
                if self.mnemonic == *from {
                    out.push(Mutation::new(
                        Operator::Mnemonic,
                        vec![self.with_mnemonic(to)],
                    ));
                }
            }
        }

        if operators.enabled(Operator::Swap) {
            if arch.is_noncommutative(&self.mnemonic)
                && n >= 3
                && Immediate::find(&self.operands[n - 1]).is_none()
            {
                out.push(Mutation::new(
                    Operator::Swap,
                    vec![self.swapped(n - 2, n - 1)],
                ));
            }
            if arch.is_pair(&self.mnemonic) && n >= 3 {
                out.push(Mutation::new(Operator::Swap, vec![self.swapped(0, 1)]));
            }
        }

        if operators.enabled(Operator::Immediate) {
            let step = if arch.is_load_store(&self.mnemonic) {
                16
            } else {
                1
            };
            for (i, operand) in self.operands.iter().enumerate() {
                if let Some(immediate) = Immediate::find(operand) {
                    for delta in [step, -step] {
                        let mut operands = self.operands.clone();
                        operands[i] = immediate.shifted(operand, delta);
                        out.push(Mutation::new(
                            Operator::Immediate,
                            vec![self.with_operands(operands)],
                        ));
                    }
                }
            }
        }

        if operators.enabled(Operator::CarryClear) {
            out.extend(arch.carry_clear(self));
        }
        if operators.enabled(Operator::CarrySet) {
            out.extend(arch.carry_set(self));
        }
        if operators.enabled(Operator::Csel) {
            out.extend(arch.select_mutations(self));
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::Aarch64;

    fn instruction(text: &str) -> Instruction {
        Instruction::parse(text).expect("an instruction")
    }

    fn describe(text: &str, operators: &Operators) -> Vec<String> {
        instruction(text)
            .mutations(&Aarch64, operators)
            .iter()
            .map(|m| format!("{}: {}", m.operator, m.describe()))
            .collect()
    }

    #[test]
    fn coverage_operators_on_a_butterfly() {
        let all = Operators::all();
        assert_eq!(
            describe("sub v22.8H, v27.8H, v18.8H", &all),
            [
                "delete: nop",
                "mnemonic: add v22.8H, v27.8H, v18.8H",
                "swap: sub v22.8H, v18.8H, v27.8H",
            ]
        );
        assert_eq!(
            describe("ldp q6, q27, [{ptr}, #0]", &all),
            [
                "delete: nop",
                "swap: ldp q27, q6, [{ptr}, #0]",
                "immediate: ldp q6, q27, [{ptr}, #16]",
                "immediate: ldp q6, q27, [{ptr}, #-16]",
            ]
        );
        assert_eq!(
            describe("sub x9, x9, 1", &all),
            [
                "delete: nop",
                "mnemonic: add x9, x9, 1",
                "immediate: sub x9, x9, 2",
                "immediate: sub x9, x9, 0",
            ]
        );
    }

    #[test]
    fn flag_operators_follow_the_carry() {
        let flags = Operators::parse(&["flags".to_string()]).unwrap();
        assert_eq!(
            describe("adcs x21, x21, x15", &flags),
            [
                "carry-clear: adds x21, x21, x15",
                "carry-set: subs xzr, xzr, xzr; adcs x21, x21, x15",
            ]
        );
        assert_eq!(
            describe("sbcs w15, w20, w6", &flags),
            [
                "carry-clear: adds wzr, wzr, wzr; sbcs w15, w20, w6",
                "carry-set: subs w15, w20, w6",
            ]
        );
        assert_eq!(
            describe("adc x23, xzr, x17", &flags),
            [
                "carry-clear: add x23, xzr, x17",
                "carry-set: add x23, xzr, x17; add x23, x23, #1",
            ]
        );
        assert_eq!(
            describe("sbc x4, x5, x6", &flags),
            [
                "carry-clear: sub x4, x5, x6; sub x4, x4, #1",
                "carry-set: sub x4, x5, x6",
            ]
        );
        // Nothing to step into a zero-register destination.
        assert_eq!(
            describe("adc xzr, x5, x6", &flags),
            ["carry-clear: add xzr, x5, x6"]
        );
        assert_eq!(
            describe("csel x19, x19, x14, lo", &flags),
            ["csel: mov x19, x19", "csel: mov x19, x14"]
        );
        assert!(describe("add v1.8H, v2.8H, v3.8H", &flags).is_empty());
    }

    #[test]
    fn operator_lists_expand_families_and_groups() {
        let parse = |tokens: &[&str]| {
            Operators::parse(&tokens.iter().map(|t| t.to_string()).collect::<Vec<_>>())
                .map(|o| o.names())
        };
        assert_eq!(
            parse(&["flags"]).unwrap(),
            ["carry-clear", "carry-set", "csel"]
        );
        assert_eq!(parse(&["carry"]).unwrap(), ["carry-clear", "carry-set"]);
        assert_eq!(parse(&["delete", "csel"]).unwrap(), ["delete", "csel"]);
        assert_eq!(parse(&[]).unwrap().len(), Operator::ALL.len());
        assert!(parse(&["bogus"]).is_err());
    }
}
