//! Everything a run needs, resolved from the command line and the config
//! file. Command-line values win.

use std::{
    env, fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::Regex;

use crate::{
    cargo::CargoArgs,
    cli::{BaselineStrategy, Cli, Shard, Sharding},
    config::Config,
    in_diff::DiffLines,
    operators::Operators,
};

/// cargo-mutants' defaults.
const DEFAULT_TIMEOUT_MULTIPLIER: f64 = 1.5;
const DEFAULT_BUILD_TIMEOUT_MULTIPLIER: f64 = 2.0;
const DEFAULT_MINIMUM_TEST_TIMEOUT: Duration = Duration::from_secs(20);
/// Used when the baseline is skipped and no timeout was given.
pub const FALLBACK_TIMEOUT: Duration = Duration::from_secs(300);

/// File globs with cargo-mutants' rule: a glob containing `/` matches the
/// whole root-relative path, any other glob matches the file name.
#[derive(Debug)]
pub struct FileGlobs {
    paths: GlobSet,
    names: GlobSet,
    empty: bool,
}

impl FileGlobs {
    pub fn new(patterns: &[String]) -> Result<Self> {
        let mut paths = GlobSetBuilder::new();
        let mut names = GlobSetBuilder::new();
        for pattern in patterns {
            let glob = Glob::new(pattern).with_context(|| format!("bad glob `{pattern}`"))?;
            if pattern.contains('/') {
                paths.add(glob);
            } else {
                names.add(glob);
            }
        }
        Ok(Self {
            paths: paths.build()?,
            names: names.build()?,
            empty: patterns.is_empty(),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.empty
    }

    pub fn matches(&self, relative: &Path) -> bool {
        self.paths.is_match(relative)
            || relative
                .file_name()
                .is_some_and(|name| self.names.is_match(name))
    }
}

#[derive(Debug)]
pub struct Options {
    /// The crate root.
    pub dir: PathBuf,
    /// Where `asm-mutants.out` goes.
    pub output_parent: PathBuf,
    pub operators: Operators,
    pub cargo: CargoArgs,
    pub examine_globs: FileGlobs,
    pub exclude_globs: FileGlobs,
    pub examine_re: Vec<Regex>,
    pub exclude_re: Vec<Regex>,
    pub in_diff: Option<DiffLines>,
    pub shard: Option<Shard>,
    pub sharding: Sharding,
    pub shuffle: bool,
    pub jobs: usize,
    pub baseline: BaselineStrategy,
    pub check_only: bool,
    pub timeout: Option<Duration>,
    pub build_timeout: Option<Duration>,
    pub timeout_multiplier: f64,
    pub build_timeout_multiplier: f64,
    pub minimum_test_timeout: Duration,
    pub copy_target: bool,
    pub copy_vcs: bool,
    pub gitignore: bool,
    pub in_place: bool,
    pub leak_dirs: bool,
    pub list: bool,
    pub list_files: bool,
    pub json: bool,
    pub diff: bool,
    pub show_caught: bool,
    pub show_unviable: bool,
    pub no_times: bool,
}

impl Options {
    pub fn new(cli: Cli) -> Result<Self> {
        let dir = match &cli.dir {
            Some(dir) => dir.clone(),
            None => env::current_dir()?,
        };
        let config = if cli.no_config {
            Config::default()
        } else {
            let path = cli
                .config
                .clone()
                .unwrap_or_else(|| dir.join(".cargo").join("asm-mutants.toml"));
            Config::read(&path)?
        };

        let operators = if cli.operators.is_empty() {
            Operators::parse(&config.operators)?
        } else {
            Operators::parse(&cli.operators)?
        };

        let features: Vec<String> = cli
            .features
            .iter()
            .flat_map(|list| list.split([',', ' ']))
            .filter(|f| !f.is_empty())
            .map(str::to_string)
            .collect();
        let mut cargo_args = config.additional_cargo_args.clone();
        cargo_args.extend(cli.cargo_arg.iter().cloned());
        let mut cargo_test_args = config.additional_cargo_test_args.clone();
        cargo_test_args.extend(cli.cargo_test_arg.iter().cloned());
        let cargo = CargoArgs {
            tool: cli.test_tool.or(config.test_tool).unwrap_or_default(),
            profile: cli.profile.clone().or(config.profile.clone()),
            features,
            all_features: cli.all_features,
            no_default_features: cli.no_default_features,
            cargo_args,
            cargo_test_args,
            test_binary_args: cli.cargo_test_args.clone(),
        };

        let regexes = |cli_patterns: &[String], config_patterns: &[String]| -> Result<Vec<Regex>> {
            cli_patterns
                .iter()
                .chain(config_patterns)
                .map(|p| Regex::new(p).with_context(|| format!("bad regex `{p}`")))
                .collect()
        };
        let in_diff = match &cli.in_diff {
            Some(path) => {
                let text = fs::read_to_string(path)
                    .with_context(|| format!("read diff {}", path.display()))?;
                Some(DiffLines::parse(&text)?)
            }
            None => None,
        };
        let seconds = |value: Option<f64>| value.map(Duration::from_secs_f64);

        Ok(Self {
            output_parent: cli
                .output
                .clone()
                .or_else(|| config.output.as_ref().map(PathBuf::from))
                .unwrap_or_else(|| dir.clone()),
            dir,
            operators,
            cargo,
            examine_globs: FileGlobs::new(
                &[cli.file.clone(), config.examine_globs.clone()].concat(),
            )?,
            exclude_globs: FileGlobs::new(
                &[cli.exclude.clone(), config.exclude_globs.clone()].concat(),
            )?,
            examine_re: regexes(&cli.examine_re, &config.examine_re)?,
            exclude_re: regexes(&cli.exclude_re, &config.exclude_re)?,
            in_diff,
            shard: cli.shard,
            sharding: cli.sharding,
            shuffle: cli.shuffle,
            jobs: cli.jobs.unwrap_or(1).max(1),
            baseline: cli.baseline,
            check_only: cli.check,
            timeout: seconds(cli.timeout.or(config.timeout)),
            build_timeout: seconds(cli.build_timeout.or(config.build_timeout)),
            timeout_multiplier: cli
                .timeout_multiplier
                .or(config.timeout_multiplier)
                .unwrap_or(DEFAULT_TIMEOUT_MULTIPLIER),
            build_timeout_multiplier: cli
                .build_timeout_multiplier
                .or(config.build_timeout_multiplier)
                .unwrap_or(DEFAULT_BUILD_TIMEOUT_MULTIPLIER),
            minimum_test_timeout: seconds(cli.minimum_test_timeout.or(config.minimum_test_timeout))
                .unwrap_or(DEFAULT_MINIMUM_TEST_TIMEOUT),
            copy_target: cli.copy_target.or(config.copy_target).unwrap_or(false),
            copy_vcs: cli.copy_vcs.or(config.copy_vcs).unwrap_or(false),
            gitignore: cli.gitignore.or(config.gitignore).unwrap_or(true),
            in_place: cli.in_place,
            leak_dirs: cli.leak_dirs,
            list: cli.list,
            list_files: cli.list_files,
            json: cli.json,
            diff: cli.diff,
            show_caught: cli.caught,
            show_unviable: cli.unviable,
            no_times: cli.no_times,
        })
    }

    /// Whether the file globs admit `relative`.
    pub fn examines_file(&self, relative: &Path) -> bool {
        (self.examine_globs.is_empty() || self.examine_globs.matches(relative))
            && !self.exclude_globs.matches(relative)
    }

    /// Whether the name regexes admit a mutant.
    pub fn examines_mutant(&self, name: &str) -> bool {
        (self.examine_re.is_empty() || self.examine_re.iter().any(|re| re.is_match(name)))
            && !self.exclude_re.iter().any(|re| re.is_match(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_with_a_slash_match_the_path() {
        let globs = FileGlobs::new(&["src/**/neon.rs".to_string(), "*.s".to_string()]).unwrap();
        assert!(globs.matches(Path::new("src/algebraic/poly/arch/neon.rs")));
        assert!(globs.matches(Path::new("src/backend/keccak.s")));
        assert!(!globs.matches(Path::new("src/algebraic/poly/arch/avx2.rs")));
        assert!(!globs.matches(Path::new("neon.rs")));
    }
}
