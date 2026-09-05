//! `asm-mutants.out/`, laid out like `mutants.out/`: the mutant list, a log
//! and diff per scenario, the four verdict lists, and `outcomes.json`.

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::{
    mutant::MutantRecord,
    outcome::{LabOutcome, ScenarioOutcome, Summary},
    timestamp::Timestamp,
};

pub const OUTPUT_NAME: &str = "asm-mutants.out";

#[derive(Serialize)]
struct Lock {
    cargo_asm_mutants_version: &'static str,
    start_time: String,
}

#[derive(Debug)]
pub struct OutputDir {
    path: PathBuf,
}

impl OutputDir {
    /// Creates `asm-mutants.out` under `parent`, moving a previous one to
    /// `asm-mutants.out.old`.
    pub fn new(parent: &Path) -> Result<Self> {
        let path = parent.join(OUTPUT_NAME);
        if path.exists() {
            let old = parent.join(format!("{OUTPUT_NAME}.old"));
            if old.exists() {
                fs::remove_dir_all(&old).with_context(|| format!("remove {}", old.display()))?;
            }
            fs::rename(&path, &old).with_context(|| format!("rotate {}", path.display()))?;
        }
        fs::create_dir_all(path.join("log"))?;
        fs::create_dir_all(path.join("diff"))?;
        for name in ["caught.txt", "missed.txt", "timeout.txt", "unviable.txt"] {
            File::create(path.join(name))?;
        }
        let lock = Lock {
            cargo_asm_mutants_version: env!("CARGO_PKG_VERSION"),
            start_time: Timestamp::now().iso8601(),
        };
        fs::write(path.join("lock.json"), serde_json::to_string_pretty(&lock)?)?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write_mutants(&self, records: &[MutantRecord]) -> Result<()> {
        let file = File::create(self.path.join("mutants.json"))?;
        serde_json::to_writer_pretty(file, records)?;
        Ok(())
    }

    /// A fresh log file `log/<stem>.log`, and its output-relative path.
    pub fn create_log(&self, stem: &str) -> Result<(File, String)> {
        let relative = format!("log/{stem}.log");
        let file = File::create(self.path.join(&relative))
            .with_context(|| format!("create {relative}"))?;
        Ok((file, relative))
    }

    /// Writes `diff/<stem>.diff`, returning its output-relative path.
    pub fn write_diff(&self, stem: &str, diff: &str) -> Result<String> {
        let relative = format!("diff/{stem}.diff");
        fs::write(self.path.join(&relative), diff)?;
        Ok(relative)
    }

    /// Appends the scenario's name to the list its verdict belongs in.
    pub fn record(&self, outcome: &ScenarioOutcome) -> Result<()> {
        let list = match outcome.summary {
            Summary::CaughtMutant => "caught.txt",
            Summary::MissedMutant => "missed.txt",
            Summary::Timeout => "timeout.txt",
            Summary::Unviable => "unviable.txt",
            Summary::Success | Summary::BaselineFailed => return Ok(()),
        };
        let mut file = OpenOptions::new().append(true).open(self.path.join(list))?;
        writeln!(file, "{}", outcome.name())?;
        Ok(())
    }

    pub fn write_outcomes(&self, lab: &LabOutcome) -> Result<()> {
        let file = File::create(self.path.join("outcomes.json"))?;
        serde_json::to_writer_pretty(file, lab)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::process::id;

    use super::*;

    #[test]
    fn rotates_a_previous_run() {
        let parent = std::env::temp_dir().join(format!("asm-mutants-output-{}", id()));
        fs::create_dir_all(&parent).unwrap();
        let first = OutputDir::new(&parent).unwrap();
        fs::write(first.path().join("marker"), "1").unwrap();
        let second = OutputDir::new(&parent).unwrap();
        assert!(!second.path().join("marker").exists());
        assert!(parent.join("asm-mutants.out.old/marker").exists());
        assert!(second.path().join("lock.json").exists());
        assert!(second.path().join("log").is_dir());
        fs::remove_dir_all(parent).unwrap();
    }
}
