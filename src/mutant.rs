//! One site with one mutation applied: its name, its diff, and the mutated
//! file.

use std::fmt::{self, Display, Formatter};

use serde::Serialize;

use crate::{
    operators::{Mutation, Operator},
    source::{Site, Syntax},
};

#[derive(Clone, Debug)]
pub struct Mutant {
    pub site: Site,
    pub mutation: Mutation,
}

/// The JSON view of a mutant, in `mutants.json` and `outcomes.json`.
#[derive(Clone, Debug, Serialize)]
pub struct MutantRecord {
    pub name: String,
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub genre: Operator,
    pub original: String,
    pub replacement: String,
    pub diff: String,
}

impl Mutant {
    /// `path:line:col: replace "X" with "Y"`, the name filters match and the
    /// list files carry.
    pub fn name(&self) -> String {
        format!(
            "{}: replace \"{}\" with \"{}\"",
            self.site,
            self.site.instruction,
            self.mutation.describe()
        )
    }

    /// A file-name stem unique to the site: `src__x.rs_line_134_col_10`.
    pub fn file_stem(&self) -> String {
        format!(
            "{}_line_{}_col_{}",
            self.site
                .file
                .path
                .to_string_lossy()
                .replace(['/', '\\'], "__"),
            self.site.line + 1,
            self.site.column
        )
    }

    /// The site's file with this mutant applied.
    pub fn apply(&self) -> String {
        let original = &self.site.file.text;
        let mut lines: Vec<String> = original.lines().map(str::to_string).collect();
        lines.splice(self.site.line..=self.site.line, self.replacement_lines());
        let mut text = lines.join("\n");
        if original.ends_with('\n') {
            text.push('\n');
        }
        text
    }

    /// The lines that replace the site's line.
    fn replacement_lines(&self) -> Vec<String> {
        let line = self
            .site
            .file
            .text
            .lines()
            .nth(self.site.line)
            .unwrap_or_default();
        let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
        let last = self.mutation.replacement.len() - 1;
        match self.site.syntax {
            Syntax::RustTemplate => {
                let suffix = line.rfind('"').map_or("", |i| &line[i + 1..]);
                // rustc rejects an `asm!` operand the template never names,
                // so a replacement that drops one keeps it in a comment.
                let described = self.mutation.describe();
                let dropped: Vec<String> = self
                    .site
                    .instruction
                    .placeholders()
                    .into_iter()
                    .filter(|p| !described.contains(p.as_str()))
                    .collect();
                self.mutation
                    .replacement
                    .iter()
                    .enumerate()
                    .map(|(i, instruction)| {
                        let comment = if i == last && !dropped.is_empty() {
                            format!(" // {}", dropped.join(" "))
                        } else {
                            String::new()
                        };
                        let end = if i == last { suffix } else { "," };
                        format!("{indent}\"{instruction}{comment}\"{end}")
                    })
                    .collect()
            }
            Syntax::Gas => self
                .mutation
                .replacement
                .iter()
                .map(|instruction| format!("{indent}{instruction}"))
                .collect(),
        }
    }

    /// A unified diff of the one changed line, cargo-mutants style.
    pub fn diff(&self) -> String {
        let original = self
            .site
            .file
            .text
            .lines()
            .nth(self.site.line)
            .unwrap_or_default();
        let replacement = self.replacement_lines();
        let mut diff = format!(
            "--- {}\n+++ replace \"{}\" with \"{}\"\n@@ -{},1 +{},{} @@\n-{original}\n",
            self.site.file.path.display(),
            self.site.instruction,
            self.mutation.describe(),
            self.site.line + 1,
            self.site.line + 1,
            replacement.len(),
        );
        for line in replacement {
            diff.push('+');
            diff.push_str(&line);
            diff.push('\n');
        }
        diff
    }

    pub fn record(&self) -> MutantRecord {
        MutantRecord {
            name: self.name(),
            file: self.site.file.path.to_string_lossy().into_owned(),
            line: self.site.line + 1,
            column: self.site.column,
            genre: self.mutation.operator,
            original: self.site.instruction.to_string(),
            replacement: self.mutation.describe(),
            diff: self.diff(),
        }
    }
}

impl Display for Mutant {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name())
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use super::*;
    use crate::{instruction::Instruction, source::SourceFile};

    fn instruction(text: &str) -> Instruction {
        Instruction::parse(text).expect("an instruction")
    }

    fn site(path: &str, text: &str, line: usize, column: usize, syntax: Syntax) -> Site {
        let file = Arc::new(SourceFile {
            path: PathBuf::from(path),
            text: text.to_string(),
        });
        let instruction = match syntax {
            Syntax::RustTemplate => {
                let raw = text.lines().nth(line).unwrap().trim();
                instruction(&raw[1..raw.rfind('"').unwrap()])
            }
            Syntax::Gas => instruction(text.lines().nth(line).unwrap()),
        };
        Site {
            file,
            line,
            column,
            syntax,
            instruction,
        }
    }

    const TEMPLATE: &str = concat!(
        "    asm!(\n",
        "        \"dup v28.8h, {zeta:w}\",\n",
        "        \"nop\"\n",
        "        zeta = in(reg) 1,\n",
        "    );\n",
    );

    #[test]
    fn names_and_diffs_like_cargo_mutants() {
        let site = site("src/k.rs", TEMPLATE, 1, 10, Syntax::RustTemplate);
        let mutant = Mutant {
            site,
            mutation: Mutation::new(Operator::Delete, vec![Instruction::nop()]),
        };
        assert_eq!(
            mutant.name(),
            "src/k.rs:2:10: replace \"dup v28.8h, {zeta:w}\" with \"nop\""
        );
        assert_eq!(mutant.file_stem(), "src__k.rs_line_2_col_10");
        assert_eq!(
            mutant.diff(),
            concat!(
                "--- src/k.rs\n",
                "+++ replace \"dup v28.8h, {zeta:w}\" with \"nop\"\n",
                "@@ -2,1 +2,1 @@\n",
                "-        \"dup v28.8h, {zeta:w}\",\n",
                "+        \"nop // {zeta:w}\",\n",
            )
        );
    }

    #[test]
    fn applies_to_a_rust_template_keeping_dropped_placeholders() {
        let site = site("src/k.rs", TEMPLATE, 1, 10, Syntax::RustTemplate);
        let two = Mutant {
            site,
            mutation: Mutation::new(
                Operator::CarrySet,
                vec![
                    instruction("subs xzr, xzr, xzr"),
                    instruction("dup v28.8h, {zeta:w}"),
                ],
            ),
        };
        let applied = two.apply();
        let lines: Vec<&str> = applied.lines().collect();
        assert_eq!(lines[1], "        \"subs xzr, xzr, xzr\",");
        assert_eq!(lines[2], "        \"dup v28.8h, {zeta:w}\",");
        assert_eq!(lines[3], "        \"nop\"");
        assert_eq!(lines.len(), 6);
        assert!(applied.ends_with('\n'));
    }

    #[test]
    fn applies_to_gas_keeping_indent() {
        let source = "_f:\n    adcs x1,x2,x3 // carry\n    ret\n";
        let mutant = Mutant {
            site: site("f.s", source, 1, 5, Syntax::Gas),
            mutation: Mutation::new(
                Operator::CarrySet,
                vec![
                    instruction("subs xzr, xzr, xzr"),
                    instruction("adcs x1, x2, x3"),
                ],
            ),
        };
        assert_eq!(
            mutant.apply(),
            "_f:\n    subs xzr, xzr, xzr\n    adcs x1, x2, x3\n    ret\n"
        );
        assert_eq!(
            mutant.name(),
            "f.s:2:5: replace \"adcs x1, x2, x3\" with \"subs xzr, xzr, xzr; adcs x1, x2, x3\""
        );
    }
}
