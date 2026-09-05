//! The source tree: which files carry assembly, and where each instruction
//! sits in them.

use std::{
    fmt::{self, Display, Formatter},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use ignore::WalkBuilder;

use crate::instruction::Instruction;

/// Directories never worth walking or copying.
pub const VCS_DIRS: &[&str] = &[".git", ".hg", ".bzr", ".svn", "_darcs", ".pijul"];

/// Our own output, and cargo-mutants', when they sit inside the tree.
pub const OUTPUT_DIRS: &[&str] = &[
    "asm-mutants.out",
    "asm-mutants.out.old",
    "mutants.out",
    "mutants.out.old",
];

/// One file in the tree, root-relative path and full contents.
#[derive(Debug, PartialEq, Eq)]
pub struct SourceFile {
    pub path: PathBuf,
    pub text: String,
}

/// How the instruction is written in its file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Syntax {
    /// A quoted line inside an `asm!` template.
    RustTemplate,
    /// A raw line in a `.s` file.
    Gas,
}

/// Where an instruction lives: the file, a zero-based line, and a one-based
/// column of its first character.
#[derive(Clone, Debug)]
pub struct Site {
    pub file: Arc<SourceFile>,
    pub line: usize,
    pub column: usize,
    pub syntax: Syntax,
    pub instruction: Instruction,
}

impl Display for Site {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}",
            self.file.path.display(),
            self.line + 1,
            self.column
        )
    }
}

/// The crate being mutated.
#[derive(Debug)]
pub struct SourceTree {
    pub root: PathBuf,
}

impl SourceTree {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Rust files under the root that use `asm!`, root-relative and sorted.
    pub fn asm_files(&self, gitignore: bool) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        let walk = WalkBuilder::new(&self.root)
            .hidden(false)
            .require_git(false)
            .git_ignore(gitignore)
            .git_global(false)
            .git_exclude(false)
            .filter_entry(|entry| {
                let name = entry.file_name().to_string_lossy();
                !(entry.depth() == 1
                    && (name == "target"
                        || VCS_DIRS.contains(&name.as_ref())
                        || OUTPUT_DIRS.contains(&name.as_ref())))
            })
            .build();
        for entry in walk {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "rs")
                && fs::read_to_string(path)
                    .with_context(|| format!("read {}", path.display()))?
                    .contains("asm!(")
            {
                files.push(path.strip_prefix(&self.root).unwrap_or(path).to_path_buf());
            }
        }
        files.sort();
        Ok(files)
    }

    pub fn read(&self, file: &Path) -> Result<Arc<SourceFile>> {
        let full = self.root.join(file);
        let text = fs::read_to_string(&full).with_context(|| format!("read {}", full.display()))?;
        Ok(Arc::new(SourceFile {
            path: file.to_path_buf(),
            text,
        }))
    }

    /// Every instruction in the file: `asm!` templates and included `.s`
    /// files for a Rust source, raw lines for an assembly file.
    pub fn sites(&self, file: &Path) -> Result<Vec<Site>> {
        self.sites_in(self.read(file)?)
    }

    /// [`Self::sites`] over already-read contents; included `.s` files are
    /// still read from the tree.
    pub fn sites_in(&self, file: Arc<SourceFile>) -> Result<Vec<Site>> {
        if file.path.extension().is_some_and(|e| e != "rs") {
            return Ok(Self::gas_sites(&file));
        }
        let mut sites = Vec::new();
        let mut in_template = false;

        for (line, raw) in file.text.lines().enumerate() {
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }
            if trimmed.contains("asm!(") {
                if let Some(included) = Self::include_str(trimmed) {
                    let dir = file.path.parent().unwrap_or(Path::new(""));
                    let included = dir.join(included);
                    // A generated file (`OUT_DIR`) is not in the tree.
                    match self.read(&included) {
                        Ok(gas) => sites.extend(Self::gas_sites(&gas)),
                        Err(error) => {
                            eprintln!("warning: skipping {}: {error:#}", included.display())
                        }
                    }
                } else {
                    in_template = true;
                }
                continue;
            }
            if !in_template {
                continue;
            }
            match Self::template_text(trimmed) {
                Some(template) => {
                    if let Some(instruction) = Instruction::parse(template) {
                        let quote = raw.find('"').unwrap_or(0);
                        sites.push(Site {
                            file: Arc::clone(&file),
                            line,
                            column: quote + 2,
                            syntax: Syntax::RustTemplate,
                            instruction,
                        });
                    }
                }
                // First non-string line: the operand list. Template over.
                None => in_template = false,
            }
        }
        Ok(sites)
    }

    fn gas_sites(file: &Arc<SourceFile>) -> Vec<Site> {
        file.text
            .lines()
            .enumerate()
            .filter_map(|(line, raw)| {
                Instruction::parse(raw).map(|instruction| Site {
                    file: Arc::clone(file),
                    line,
                    column: raw.len() - raw.trim_start().len() + 1,
                    syntax: Syntax::Gas,
                    instruction,
                })
            })
            .collect()
    }

    /// The path inside `include_str!("...")`, if the line has one.
    fn include_str(line: &str) -> Option<&str> {
        let start = line.find("include_str!(\"")? + "include_str!(\"".len();
        let end = line[start..].find('"')? + start;
        Some(&line[start..end])
    }

    /// The contents of a `"..."` template line.
    fn template_text(line: &str) -> Option<&str> {
        let rest = line.strip_prefix('"')?;
        let end = rest.rfind('"')?;
        Some(&rest[..end])
    }
}

#[cfg(test)]
mod tests {
    use std::process::id;

    use super::*;

    #[test]
    fn finds_template_lines_and_included_files() {
        let root = std::env::temp_dir().join(format!("asm-mutants-source-{}", id()));
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("k.rs"),
            concat!(
                "//! Not a site: `global_asm!(include_str!(\"missing.s\"))`.\n",
                "fn kernel() {\n",
                "    // asm!( in a comment is not a site either.\n",
                "    core::arch::asm!(\n",
                "        // prologue\n",
                "        \"mov x9, #4\",\n",
                "    \"2:\",\n",
                "        \"sub x9, x9, 1\",\n",
                "        \"cbnz x9, 2b\",\n",
                "        out(\"x9\") _,\n",
                "        options(nostack),\n",
                "    );\n",
                "}\n",
                "core::arch::global_asm!(include_str!(\"k.s\"));\n",
            ),
        )
        .unwrap();
        fs::write(src.join("k.s"), ".text\n_f:\n    adc x1, x2, x3\n    ret\n").unwrap();
        fs::write(root.join("plain.rs"), "fn main() {}\n").unwrap();

        let tree = SourceTree::new(root.clone());
        assert_eq!(tree.asm_files(false).unwrap(), [PathBuf::from("src/k.rs")]);

        let sites = tree.sites(Path::new("src/k.rs")).unwrap();
        let listed: Vec<String> = sites
            .iter()
            .map(|s| format!("{s} {}", s.instruction))
            .collect();
        assert_eq!(
            listed,
            [
                "src/k.rs:6:10 mov x9, #4",
                "src/k.rs:8:10 sub x9, x9, 1",
                "src/k.rs:9:10 cbnz x9, 2b",
                "src/k.s:3:5 adc x1, x2, x3",
                "src/k.s:4:5 ret",
            ]
        );
        fs::remove_dir_all(root).unwrap();
    }
}
