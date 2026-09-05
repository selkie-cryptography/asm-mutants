//! Cargo invocations: the build and test commands, run with a timeout and a
//! log.

#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::{
    env,
    ffi::OsString,
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread::sleep,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use clap::ValueEnum;
use serde::Deserialize;

use crate::outcome::{Phase, PhaseResult, ProcessStatus};

/// Tool used to run test suites.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestTool {
    #[default]
    Cargo,
    Nextest,
}

/// How every cargo command in a run is put together.
#[derive(Clone, Debug, Default)]
pub struct CargoArgs {
    pub tool: TestTool,
    pub profile: Option<String>,
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    /// Added to every invocation.
    pub cargo_args: Vec<String>,
    /// Added to test invocations only.
    pub cargo_test_args: Vec<String>,
    /// Passed to the test binaries after `--`.
    pub test_binary_args: Vec<String>,
}

impl CargoArgs {
    fn prefix(&self) -> &'static [&'static str] {
        match self.tool {
            TestTool::Cargo => &["test"],
            TestTool::Nextest => &["nextest", "run"],
        }
    }

    /// Profile, features, and the shared extra args.
    fn common(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(profile) = &self.profile {
            let flag = match self.tool {
                TestTool::Cargo => "--profile",
                TestTool::Nextest => "--cargo-profile",
            };
            args.push(flag.to_string());
            args.push(profile.clone());
        }
        if !self.features.is_empty() {
            args.push("--features".to_string());
            args.push(self.features.join(","));
        }
        if self.all_features {
            args.push("--all-features".to_string());
        }
        if self.no_default_features {
            args.push("--no-default-features".to_string());
        }
        args.extend(self.cargo_args.iter().cloned());
        args
    }

    /// `cargo test --no-run` or `cargo nextest run --no-run`.
    pub fn build_argv(&self) -> Vec<String> {
        let mut argv: Vec<String> = self.prefix().iter().map(|s| s.to_string()).collect();
        argv.push("--no-run".to_string());
        argv.extend(self.common());
        argv
    }

    /// `cargo test` or `cargo nextest run`, with the test-only args.
    pub fn test_argv(&self) -> Vec<String> {
        let mut argv: Vec<String> = self.prefix().iter().map(|s| s.to_string()).collect();
        argv.extend(self.common());
        argv.extend(self.cargo_test_args.iter().cloned());
        if !self.test_binary_args.is_empty() {
            argv.push("--".to_string());
            argv.extend(self.test_binary_args.iter().cloned());
        }
        argv
    }
}

/// One cargo command in one directory.
#[derive(Clone, Debug)]
pub struct Invocation {
    pub argv: Vec<String>,
    pub cwd: PathBuf,
}

impl Invocation {
    pub fn new(argv: Vec<String>, cwd: &Path) -> Self {
        Self {
            argv,
            cwd: cwd.to_path_buf(),
        }
    }

    /// The cargo binary: the one that invoked us, or `cargo` on the path.
    fn cargo() -> OsString {
        env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
    }

    /// Runs to completion or `timeout`, with output appended to `log`.
    pub fn run(&self, phase: Phase, log: &mut File, timeout: Duration) -> Result<PhaseResult> {
        let cargo = Self::cargo();
        writeln!(
            log,
            "\n$ {} {}",
            cargo.to_string_lossy(),
            self.argv.join(" ")
        )?;
        let mut command = Command::new(&cargo);
        command
            .args(&self.argv)
            .current_dir(&self.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log.try_clone()?));
        #[cfg(unix)]
        command.process_group(0);

        let start = Instant::now();
        let mut child = command.spawn().with_context(|| {
            format!("spawn {} {}", cargo.to_string_lossy(), self.argv.join(" "))
        })?;
        let process_status = loop {
            if let Some(status) = child.try_wait()? {
                break ProcessStatus::from(status);
            }
            if start.elapsed() > timeout {
                Self::kill(&mut child)?;
                break ProcessStatus::Timeout;
            }
            sleep(Duration::from_millis(50));
        };
        let mut argv = vec![cargo.to_string_lossy().into_owned()];
        argv.extend(self.argv.iter().cloned());
        Ok(PhaseResult {
            phase,
            duration: start.elapsed().as_secs_f64(),
            process_status,
            argv,
        })
    }

    /// Kills the whole process group, so a spinning test binary dies with
    /// its cargo parent.
    #[cfg(unix)]
    fn kill(child: &mut Child) -> Result<()> {
        Command::new("kill")
            .args(["-9", &format!("-{}", child.id())])
            .status()?;
        child.wait()?;
        Ok(())
    }

    #[cfg(not(unix))]
    fn kill(child: &mut Child) -> Result<()> {
        child.kill()?;
        child.wait()?;
        Ok(())
    }
}

impl From<ExitStatus> for ProcessStatus {
    fn from(status: ExitStatus) -> Self {
        if status.success() {
            Self::Success
        } else {
            match status.code() {
                Some(code) => Self::Failure(code),
                None => Self::Signalled,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_cargo_and_nextest_command_lines() {
        let args = CargoArgs {
            tool: TestTool::Nextest,
            profile: Some("mutants".to_string()),
            features: vec!["expose-internals".to_string()],
            cargo_args: vec!["--lib".to_string()],
            cargo_test_args: vec!["-E".to_string(), "test(/neon/)".to_string()],
            test_binary_args: vec!["--nocapture".to_string()],
            ..CargoArgs::default()
        };
        assert_eq!(
            args.build_argv(),
            [
                "nextest",
                "run",
                "--no-run",
                "--cargo-profile",
                "mutants",
                "--features",
                "expose-internals",
                "--lib"
            ]
        );
        assert_eq!(
            args.test_argv(),
            [
                "nextest",
                "run",
                "--cargo-profile",
                "mutants",
                "--features",
                "expose-internals",
                "--lib",
                "-E",
                "test(/neon/)",
                "--",
                "--nocapture"
            ]
        );

        let plain = CargoArgs {
            all_features: true,
            ..CargoArgs::default()
        };
        assert_eq!(plain.build_argv(), ["test", "--no-run", "--all-features"]);
        assert_eq!(plain.test_argv(), ["test", "--all-features"]);
    }
}
