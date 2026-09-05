//! One line of assembly, and the immediates inside it.

use std::fmt::{self, Display, Formatter};

/// One assembly instruction: mnemonic plus bracket-aware operands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instruction {
    pub mnemonic: String,
    pub operands: Vec<String>,
}

impl Instruction {
    /// Parses one line of assembly. `None` for labels, directives,
    /// preprocessor lines, and blanks.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.split("//").next().unwrap_or("").trim();
        if text.is_empty() || text.ends_with(':') || text.starts_with(['.', '#']) {
            return None;
        }
        let (mnemonic, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
        Some(Self {
            mnemonic: mnemonic.to_ascii_lowercase(),
            operands: Self::split_operands(rest),
        })
    }

    pub fn new(mnemonic: &str, operands: &[&str]) -> Self {
        Self {
            mnemonic: mnemonic.to_string(),
            operands: operands.iter().map(|s| s.to_string()).collect(),
        }
    }

    pub fn nop() -> Self {
        Self::new("nop", &[])
    }

    /// Splits on commas outside `[...]` and `{...}`.
    fn split_operands(rest: &str) -> Vec<String> {
        let mut operands = Vec::new();
        let mut depth = 0usize;
        let mut current = String::new();
        for c in rest.chars() {
            match c {
                '[' | '{' => {
                    depth += 1;
                    current.push(c);
                }
                ']' | '}' => {
                    depth = depth.saturating_sub(1);
                    current.push(c);
                }
                ',' if depth == 0 => {
                    operands.push(current.trim().to_string());
                    current.clear();
                }
                _ => current.push(c),
            }
        }
        if !current.trim().is_empty() {
            operands.push(current.trim().to_string());
        }
        operands
    }

    pub fn with_mnemonic(&self, mnemonic: &str) -> Self {
        Self {
            mnemonic: mnemonic.to_string(),
            operands: self.operands.clone(),
        }
    }

    pub fn with_operands(&self, operands: Vec<String>) -> Self {
        Self {
            mnemonic: self.mnemonic.clone(),
            operands,
        }
    }

    pub fn swapped(&self, i: usize, j: usize) -> Self {
        let mut operands = self.operands.clone();
        operands.swap(i, j);
        self.with_operands(operands)
    }

    /// The `{name}` / `{name:w}` template operands this instruction names.
    pub fn placeholders(&self) -> Vec<String> {
        let text = self.to_string();
        let mut out = Vec::new();
        let mut rest = text.as_str();
        while let Some(start) = rest.find('{') {
            let Some(len) = rest[start..].find('}') else {
                break;
            };
            out.push(rest[start..=start + len].to_string());
            rest = &rest[start + len + 1..];
        }
        out
    }
}

impl Display for Instruction {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if self.operands.is_empty() {
            write!(f, "{}", self.mnemonic)
        } else {
            write!(f, "{} {}", self.mnemonic, self.operands.join(", "))
        }
    }
}

/// A decimal immediate inside an operand: `#288`, `#-16`, or a bare `1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Immediate {
    start: usize,
    end: usize,
    pub value: i64,
}

impl Immediate {
    pub fn find(operand: &str) -> Option<Self> {
        if let Ok(value) = operand.parse::<i64>() {
            return Some(Self {
                start: 0,
                end: operand.len(),
                value,
            });
        }
        let start = operand.find('#')? + 1;
        let digits = &operand[start..];
        let len = digits
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '-')
            .count();
        if len == 0 || digits[len..].starts_with(['x', 'X']) {
            return None;
        }
        let value = digits[..len].parse().ok()?;
        Some(Self {
            start,
            end: start + len,
            value,
        })
    }

    /// The operand with the immediate moved by `delta`.
    pub fn shifted(&self, operand: &str, delta: i64) -> String {
        format!(
            "{}{}{}",
            &operand[..self.start],
            self.value + delta,
            &operand[self.end..]
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instruction(text: &str) -> Instruction {
        Instruction::parse(text).expect("an instruction")
    }

    #[test]
    fn parses_operands_inside_brackets_and_braces() {
        let ldp = instruction("ldp q16, q17, [{ptr}, #288] // comment");
        assert_eq!(ldp.mnemonic, "ldp");
        assert_eq!(ldp.operands, ["q16", "q17", "[{ptr}, #288]"]);

        let post = instruction("stp q22, q4, [{ptr}], #32");
        assert_eq!(post.operands, ["q22", "q4", "[{ptr}]", "#32"]);

        let dup = instruction("DUP     v28.8h, {zeta:w}");
        assert_eq!(dup.mnemonic, "dup");
        assert_eq!(dup.operands, ["v28.8h", "{zeta:w}"]);
        assert_eq!(dup.to_string(), "dup v28.8h, {zeta:w}");
        assert_eq!(dup.placeholders(), ["{zeta:w}"]);
    }

    #[test]
    fn skips_labels_directives_preprocessor_and_blanks() {
        for text in [
            "2:",
            "_func:",
            ".text",
            ".globl _f",
            "#define X 1",
            "",
            "  // comment",
        ] {
            assert!(Instruction::parse(text).is_none(), "{text:?}");
        }
        assert_eq!(instruction("nop").to_string(), "nop");
    }

    #[test]
    fn finds_immediates() {
        let imm = Immediate::find("#288").unwrap();
        assert_eq!(
            (imm.value, imm.shifted("#288", 16)),
            (288, "#304".to_string())
        );

        let bare = Immediate::find("1").unwrap();
        assert_eq!(bare.shifted("1", -1), "0");

        let mem = Immediate::find("[{ptr}, #-16]").unwrap();
        assert_eq!(mem.shifted("[{ptr}, #-16]", 16), "[{ptr}, #0]");

        for operand in ["x9", "v28.8h", "{zeta:w}", "2b", "#0x10", "[{ptr}]"] {
            assert!(Immediate::find(operand).is_none(), "{operand}");
        }
    }
}
