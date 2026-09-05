//! `.cargo/asm-mutants.toml`, the same keys as cargo-mutants' `mutants.toml`
//! where they apply, plus `operators`.

use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::cargo::TestTool;

/// The configuration file. Every key is optional; command-line options win.
#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Additional args for all cargo invocations.
    pub additional_cargo_args: Vec<String>,
    /// Additional args for cargo test.
    pub additional_cargo_test_args: Vec<String>,
    /// Maximum run time for the build command, in seconds.
    pub build_timeout: Option<f64>,
    /// Build timeout multiplier (relative to base build time).
    pub build_timeout_multiplier: Option<f64>,
    /// Copy the `target` directory to build directories.
    pub copy_target: Option<bool>,
    /// Copy `.git` and other VCS directories to the build directory.
    pub copy_vcs: Option<bool>,
    /// Globs for files to examine.
    pub examine_globs: Vec<String>,
    /// Globs for files to exclude.
    pub exclude_globs: Vec<String>,
    /// Regexes for mutants to examine, matched against `--list` names.
    pub examine_re: Vec<String>,
    /// Regexes for mutants to exclude, matched against `--list` names.
    pub exclude_re: Vec<String>,
    /// Don't copy files matching gitignore patterns.
    pub gitignore: Option<bool>,
    /// Minimum timeout for tests, in seconds.
    pub minimum_test_timeout: Option<f64>,
    /// Operators or families to apply.
    pub operators: Vec<String>,
    /// Create `asm-mutants.out` within this directory.
    pub output: Option<String>,
    /// Build with this cargo profile.
    pub profile: Option<String>,
    /// Tool used to run test suites.
    pub test_tool: Option<TestTool>,
    /// Maximum run time for the test command, in seconds.
    pub timeout: Option<f64>,
    /// Test timeout multiplier (relative to base test time).
    pub timeout_multiplier: Option<f64>,
}

impl Config {
    /// Reads the file, or the default when it does not exist.
    pub fn read(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text =
            fs::read_to_string(path).with_context(|| format!("read config {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parse config {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_keys_cargo_mutants_uses() {
        let config: Config = toml::from_str(
            r#"
            test_tool = "nextest"
            profile = "mutants"
            copy_target = true
            additional_cargo_args = ["--features", "expose-internals"]
            timeout_multiplier = 2.0
            minimum_test_timeout = 90
            exclude_re = ["stp .* \\[\\{ptr\\}\\], #32"]
            operators = ["coverage"]
            "#,
        )
        .unwrap();
        assert_eq!(config.test_tool, Some(TestTool::Nextest));
        assert_eq!(config.profile.as_deref(), Some("mutants"));
        assert_eq!(config.copy_target, Some(true));
        assert_eq!(
            config.additional_cargo_args,
            ["--features", "expose-internals"]
        );
        assert_eq!(config.timeout_multiplier, Some(2.0));
        assert_eq!(config.minimum_test_timeout, Some(90.0));
        assert_eq!(config.exclude_re.len(), 1);
        assert_eq!(config.operators, ["coverage"]);
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(toml::from_str::<Config>("exclude_regex = []").is_err());
    }
}
