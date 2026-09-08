//! Command line, mirroring cargo-mutants' option names and groups.

use std::{env, ffi::OsString, path::PathBuf, str::FromStr};

use clap::{ArgAction, Parser, ValueEnum};

use crate::cargo::TestTool;

/// Whether to test the unmutated tree before any mutant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum BaselineStrategy {
    /// Run tests in an unmutated tree before testing mutants.
    Run,
    /// Don't run tests in an unmutated tree: assume that they pass.
    Skip,
}

/// How `--shard` splits the mutant list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Sharding {
    /// Run mutant `i` on shard `i % k`.
    RoundRobin,
    /// Run consecutive ranges of mutants on each shard: the first `n/k` on
    /// shard 0, etc.
    Slice,
}

/// One shard `k` of `n`, from `--shard k/n`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shard {
    pub k: usize,
    pub n: usize,
}

impl FromStr for Shard {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (k, n) = text
            .split_once('/')
            .ok_or_else(|| format!("expected K/N, got `{text}`"))?;
        let k: usize = k.parse().map_err(|_| format!("bad shard index `{k}`"))?;
        let n: usize = n.parse().map_err(|_| format!("bad shard count `{n}`"))?;
        if n == 0 || k >= n {
            return Err(format!("shard {k}/{n} is out of range"));
        }
        Ok(Self { k, n })
    }
}

/// Find inadequately-tested inline assembly.
#[derive(Debug, Parser)]
#[command(
    name = "cargo-asm-mutants",
    bin_name = "cargo asm-mutants",
    version,
    about,
    disable_version_flag = true,
    after_help = "See <https://github.com/selkie-cryptography/asm-mutants> for more information."
)]
pub struct Cli {
    /// Pass remaining arguments to cargo test after all options and after
    /// `--`.
    #[arg(last = true)]
    pub cargo_test_args: Vec<String>,

    /// Show version and quit.
    // Long-only, so `-V` can mean `--unviable` as in cargo-mutants.
    #[arg(long, action = ArgAction::Version, help_heading = "Meta")]
    pub version: (),

    /// Build with this cargo profile.
    #[arg(long, help_heading = "Build")]
    pub profile: Option<String>,

    /// Read configuration from this file instead of
    /// `.cargo/asm-mutants.toml`.
    #[arg(long, help_heading = "Config")]
    pub config: Option<PathBuf>,

    /// Don't read `.cargo/asm-mutants.toml`.
    #[arg(long, help_heading = "Config")]
    pub no_config: bool,

    /// Copy the `target` directory to build directories.
    #[arg(long, help_heading = "Copying")]
    pub copy_target: Option<bool>,

    /// Copy `.git` and other VCS directories to the build directory.
    #[arg(long, help_heading = "Copying")]
    pub copy_vcs: Option<bool>,

    /// Don't copy files matching gitignore patterns.
    #[arg(long, help_heading = "Copying")]
    pub gitignore: Option<bool>,

    /// Test mutations in the source tree, rather than in a copy.
    #[arg(long, help_heading = "Copying")]
    pub in_place: bool,

    /// Don't delete the scratch directories, for debugging.
    #[arg(long, help_heading = "Debug")]
    pub leak_dirs: bool,

    /// Baseline strategy: check that tests pass in an unmutated tree before
    /// testing mutants.
    #[arg(long, value_enum, default_value_t = BaselineStrategy::Run, help_heading = "Execution")]
    pub baseline: BaselineStrategy,

    /// Maximum run time for the build command, in seconds.
    #[arg(long, help_heading = "Execution")]
    pub build_timeout: Option<f64>,

    /// Build timeout multiplier (relative to base build time).
    #[arg(long, help_heading = "Execution")]
    pub build_timeout_multiplier: Option<f64>,

    /// Additional args for all cargo invocations.
    #[arg(
        short = 'C',
        long,
        allow_hyphen_values = true,
        help_heading = "Execution"
    )]
    pub cargo_arg: Vec<String>,

    /// Additional args for cargo test.
    #[arg(long, allow_hyphen_values = true, help_heading = "Execution")]
    pub cargo_test_arg: Vec<String>,

    /// Build generated mutants, but don't run tests.
    #[arg(long, help_heading = "Execution")]
    pub check: bool,

    /// Run this many build/test jobs in parallel.
    #[arg(
        short,
        long,
        env = "CARGO_ASM_MUTANTS_JOBS",
        help_heading = "Execution"
    )]
    pub jobs: Option<usize>,

    /// Just list possible mutants, don't run them.
    #[arg(long, help_heading = "Execution")]
    pub list: bool,

    /// List source files with inline assembly, don't run anything.
    #[arg(long, help_heading = "Execution")]
    pub list_files: bool,

    /// Minimum timeout for tests, in seconds, as a lower bound on the
    /// auto-set time.
    #[arg(
        long,
        env = "CARGO_ASM_MUTANTS_MINIMUM_TEST_TIMEOUT",
        help_heading = "Execution"
    )]
    pub minimum_test_timeout: Option<f64>,

    /// Run only one shard of all generated mutants: specify as e.g. 1/4.
    #[arg(long, help_heading = "Execution")]
    pub shard: Option<Shard>,

    /// Sharding method to use with `--shard`.
    #[arg(long, value_enum, default_value_t = Sharding::Slice, help_heading = "Execution")]
    pub sharding: Sharding,

    /// Run mutants in random order.
    ///
    /// Randomization occurs after sharding: each shard will run its assigned
    /// mutants in random order.
    #[arg(long, conflicts_with = "no_shuffle", help_heading = "Execution")]
    pub shuffle: bool,

    /// Run mutants in the fixed order they occur in the source tree.
    ///
    /// This is the default behavior.
    #[arg(long, help_heading = "Execution")]
    pub no_shuffle: bool,

    /// Maximum run time for the test command, in seconds.
    #[arg(short = 't', long, help_heading = "Execution")]
    pub timeout: Option<f64>,

    /// Test timeout multiplier (relative to base test time).
    #[arg(long, help_heading = "Execution")]
    pub timeout_multiplier: Option<f64>,

    /// Tool used to run test suites: cargo or nextest.
    #[arg(long, value_enum, help_heading = "Execution")]
    pub test_tool: Option<TestTool>,

    /// Space or comma separated list of features to activate.
    #[arg(long, help_heading = "Features")]
    pub features: Vec<String>,

    /// Do not activate the `default` feature.
    #[arg(long, help_heading = "Features")]
    pub no_default_features: bool,

    /// Activate all features.
    #[arg(long, help_heading = "Features")]
    pub all_features: bool,

    /// Regex for mutations to examine, matched against the names shown by
    /// `--list`.
    #[arg(short = 'F', long = "re", help_heading = "Filters")]
    pub examine_re: Vec<String>,

    /// Glob for files to exclude; with no glob, all files are included;
    /// globs containing slash match the entire path.
    #[arg(short = 'e', long = "exclude", help_heading = "Filters")]
    pub exclude: Vec<String>,

    /// Regex for mutations to exclude, matched against the names shown by
    /// `--list`.
    #[arg(short = 'E', long, help_heading = "Filters")]
    pub exclude_re: Vec<String>,

    /// Glob for files to examine; with no glob, all files are examined;
    /// globs containing slash match the entire path.
    #[arg(short = 'f', long = "file", help_heading = "Filters")]
    pub file: Vec<String>,

    /// Include only mutants in code touched by this diff.
    #[arg(short = 'D', long, help_heading = "Filters")]
    pub in_diff: Option<PathBuf>,

    /// Operators or families to apply: `coverage` (delete, mnemonic, swap,
    /// immediate), `flags` (carry, csel), or a comma-separated list of
    /// operator names. Default: all.
    #[arg(long, value_delimiter = ',', help_heading = "Generate")]
    pub operators: Vec<String>,

    /// Rust crate directory to examine.
    #[arg(short = 'd', long, help_heading = "Input")]
    pub dir: Option<PathBuf>,

    /// Print mutants that were caught by tests.
    #[arg(short = 'v', long, help_heading = "Output")]
    pub caught: bool,

    /// Emit diffs showing the mutations, within `--list`.
    #[arg(long, help_heading = "Output")]
    pub diff: bool,

    /// Output json (only for `--list`).
    #[arg(long, help_heading = "Output")]
    pub json: bool,

    /// Don't print times, to make output deterministic.
    #[arg(long, help_heading = "Output")]
    pub no_times: bool,

    /// Create `asm-mutants.out` within this directory.
    #[arg(
        short = 'o',
        long,
        env = "CARGO_ASM_MUTANTS_OUTPUT",
        help_heading = "Output"
    )]
    pub output: Option<PathBuf>,

    /// Print mutations that failed to build.
    #[arg(short = 'V', long, help_heading = "Output")]
    pub unviable: bool,
}

impl Cli {
    /// Parses the process arguments, dropping the `asm-mutants` word cargo
    /// inserts when invoked as `cargo asm-mutants`.
    pub fn from_env() -> Self {
        let mut args: Vec<OsString> = env::args_os().collect();
        if args.get(1).is_some_and(|arg| arg == "asm-mutants") {
            args.remove(1);
        }
        Self::try_parse_from(args).unwrap_or_else(|error| {
            // Clap defaults to exit code 2 for usage errors; that code means
            // missed mutants here. Help and version requests still succeed.
            let code = if error.use_stderr() { 1 } else { 0 };
            let _ = error.print();
            std::process::exit(code)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shards() {
        assert_eq!("1/4".parse::<Shard>(), Ok(Shard { k: 1, n: 4 }));
        assert!("4/4".parse::<Shard>().is_err());
        assert!("1".parse::<Shard>().is_err());
        assert!("a/b".parse::<Shard>().is_err());
    }

    #[test]
    fn strips_the_cargo_subcommand_word() {
        let cli = Cli::parse_from(["cargo-asm-mutants", "--list", "-F", "adc"]);
        assert!(cli.list);
        assert_eq!(cli.examine_re, ["adc"]);
        let cli = Cli::parse_from([
            "cargo-asm-mutants",
            "--operators",
            "flags,delete",
            "--",
            "--nocapture",
        ]);
        assert_eq!(cli.operators, ["flags", "delete"]);
        assert_eq!(cli.cargo_test_args, ["--nocapture"]);
    }

    #[test]
    fn cargo_args_may_start_with_a_hyphen() {
        let cli = Cli::parse_from([
            "cargo-asm-mutants",
            "-C",
            "--lib",
            "--cargo-test-arg",
            "--release",
        ]);
        assert_eq!(cli.cargo_arg, ["--lib"]);
        assert_eq!(cli.cargo_test_arg, ["--release"]);
    }
}
