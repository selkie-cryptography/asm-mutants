//! `--in-diff`: the lines a unified diff touches, per file.

use std::{collections::HashMap, path::Path};

use anyhow::{Context, Result, bail};

/// New-side line ranges (one-based, inclusive) of every hunk, keyed by path.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DiffLines {
    ranges: HashMap<String, Vec<(usize, usize)>>,
}

impl DiffLines {
    /// Parses a unified diff as `git diff` and `diff -u` produce it.
    pub fn parse(text: &str) -> Result<Self> {
        let mut ranges: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
        // The file the following hunks belong to; empty for a deleted file,
        // whose hunks have no new side.
        let mut current: Option<String> = None;
        for line in text.lines() {
            if let Some(path) = line.strip_prefix("+++ ") {
                let path = path.split('\t').next().unwrap_or(path).trim();
                let path = path.strip_prefix("b/").unwrap_or(path);
                current = Some(if path == "/dev/null" {
                    String::new()
                } else {
                    path.to_string()
                });
            } else if let Some(hunk) = line.strip_prefix("@@ ") {
                let Some(path) = &current else {
                    bail!("hunk before any `+++` header: {line}");
                };
                if path.is_empty() {
                    continue;
                }
                let new = hunk
                    .split_whitespace()
                    .find_map(|word| word.strip_prefix('+'))
                    .with_context(|| format!("hunk header without a `+` range: {line}"))?;
                let (start, count) = match new.split_once(',') {
                    Some((start, count)) => (start.parse()?, count.parse()?),
                    None => (new.parse()?, 1usize),
                };
                if count > 0 {
                    ranges
                        .entry(path.clone())
                        .or_default()
                        .push((start, start + count - 1));
                }
            }
        }
        Ok(Self { ranges })
    }

    /// Whether the diff touches `line` (one-based) of `path`.
    pub fn contains(&self, path: &Path, line: usize) -> bool {
        self.ranges
            .get(&path.to_string_lossy().replace('\\', "/"))
            .is_some_and(|ranges| ranges.iter().any(|(a, b)| (*a..=*b).contains(&line)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = concat!(
        "diff --git a/src/k.rs b/src/k.rs\n",
        "--- a/src/k.rs\n",
        "+++ b/src/k.rs\n",
        "@@ -10,3 +10,4 @@ fn kernel() {\n",
        "     a\n",
        "+    b\n",
        "     c\n",
        "     d\n",
        "@@ -40 +41 @@\n",
        "-x\n",
        "+y\n",
        "--- a/gone.rs\n",
        "+++ /dev/null\n",
        "@@ -1,2 +0,0 @@\n",
        "-gone\n",
        "-gone\n",
    );

    #[test]
    fn keeps_hunk_ranges_on_the_new_side() {
        let lines = DiffLines::parse(DIFF).unwrap();
        let k = Path::new("src/k.rs");
        assert!(lines.contains(k, 10));
        assert!(lines.contains(k, 13));
        assert!(!lines.contains(k, 14));
        assert!(lines.contains(k, 41));
        assert!(!lines.contains(k, 40));
        assert!(!lines.contains(Path::new("gone.rs"), 1));
    }
}
