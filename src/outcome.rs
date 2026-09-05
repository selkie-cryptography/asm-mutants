//! What happened to one scenario and to the whole run, in the shape
//! cargo-mutants writes to `outcomes.json`.

use serde::Serialize;

use crate::{mutant::MutantRecord, timestamp::Timestamp};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Phase {
    Build,
    Test,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum ProcessStatus {
    Success,
    Failure(i32),
    Timeout,
    Signalled,
}

#[derive(Clone, Debug, Serialize)]
pub struct PhaseResult {
    pub phase: Phase,
    /// Seconds.
    pub duration: f64,
    pub process_status: ProcessStatus,
    pub argv: Vec<String>,
}

/// The unmutated tree, or one mutant.
#[derive(Clone, Debug, Serialize)]
pub enum Scenario {
    Baseline,
    Mutant(MutantRecord),
}

/// The verdict on one scenario.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Summary {
    /// The baseline passed, or a `--check` mutant built.
    Success,
    CaughtMutant,
    MissedMutant,
    /// The build failed.
    Unviable,
    /// A build or test ran past its timeout.
    Timeout,
    /// The baseline's tests failed.
    BaselineFailed,
}

impl Summary {
    /// Reads the verdict off the phases that ran. `tested` says whether a
    /// passing build was followed by tests at all (`--check` skips them).
    fn from_phases(baseline: bool, phases: &[PhaseResult], tested: bool) -> Self {
        for result in phases {
            match (result.phase, result.process_status) {
                (_, ProcessStatus::Timeout) => return Self::Timeout,
                (Phase::Build, ProcessStatus::Success) => {}
                (Phase::Build, _) => return Self::Unviable,
                (Phase::Test, ProcessStatus::Success) => {}
                (Phase::Test, _) if baseline => return Self::BaselineFailed,
                (Phase::Test, _) => return Self::CaughtMutant,
            }
        }
        if baseline || !tested {
            Self::Success
        } else {
            Self::MissedMutant
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Success => "ok",
            Self::CaughtMutant => "caught",
            Self::MissedMutant => "MISSED",
            Self::Unviable => "UNVIABLE",
            Self::Timeout => "TIMEOUT",
            Self::BaselineFailed => "FAILED",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ScenarioOutcome {
    pub scenario: Scenario,
    pub summary: Summary,
    /// Relative to the output directory.
    pub log_path: String,
    pub diff_path: Option<String>,
    pub phase_results: Vec<PhaseResult>,
}

impl ScenarioOutcome {
    pub fn new(
        scenario: Scenario,
        log_path: String,
        diff_path: Option<String>,
        phase_results: Vec<PhaseResult>,
        tested: bool,
    ) -> Self {
        let baseline = matches!(scenario, Scenario::Baseline);
        Self {
            summary: Summary::from_phases(baseline, &phase_results, tested),
            scenario,
            log_path,
            diff_path,
            phase_results,
        }
    }

    pub fn name(&self) -> &str {
        match &self.scenario {
            Scenario::Baseline => "Unmutated baseline",
            Scenario::Mutant(record) => &record.name,
        }
    }

    /// Seconds spent in `phase`, if it ran.
    pub fn duration(&self, phase: Phase) -> Option<f64> {
        self.phase_results
            .iter()
            .find(|r| r.phase == phase)
            .map(|r| r.duration)
    }
}

/// The process exit code, as cargo-mutants defines them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitCode {
    /// Every mutant was caught, or `--list` ran.
    Success = 0,
    /// Usage or internal error.
    Error = 1,
    /// Some mutants were missed.
    Missed = 2,
    /// Some scenarios timed out.
    Timeout = 3,
    /// The unmutated tree fails to build or test, so nothing was tested.
    BaselineFailed = 4,
}

impl From<ExitCode> for i32 {
    fn from(code: ExitCode) -> Self {
        code as Self
    }
}

/// Every outcome of a run, plus totals.
#[derive(Clone, Debug, Serialize)]
pub struct LabOutcome {
    pub outcomes: Vec<ScenarioOutcome>,
    pub total_mutants: usize,
    pub missed: usize,
    pub caught: usize,
    pub timeout: usize,
    pub unviable: usize,
    pub success: usize,
    pub start_time: String,
    pub end_time: String,
    pub cargo_asm_mutants_version: String,
}

impl LabOutcome {
    pub fn new(total_mutants: usize) -> Self {
        Self {
            outcomes: Vec::new(),
            total_mutants,
            missed: 0,
            caught: 0,
            timeout: 0,
            unviable: 0,
            success: 0,
            start_time: Timestamp::now().iso8601(),
            end_time: String::new(),
            cargo_asm_mutants_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Counts the outcome; the baseline is recorded but not tallied.
    pub fn add(&mut self, outcome: ScenarioOutcome) {
        match outcome.summary {
            Summary::Success if matches!(outcome.scenario, Scenario::Baseline) => {}
            Summary::Success => self.success += 1,
            Summary::CaughtMutant => self.caught += 1,
            Summary::MissedMutant => self.missed += 1,
            Summary::Unviable => self.unviable += 1,
            Summary::Timeout => self.timeout += 1,
            Summary::BaselineFailed => {}
        }
        self.outcomes.push(outcome);
    }

    pub fn finish(&mut self) {
        self.end_time = Timestamp::now().iso8601();
    }

    /// Timeouts outrank misses, as in cargo-mutants.
    pub fn exit_code(&self) -> ExitCode {
        if self
            .outcomes
            .iter()
            .any(|o| o.summary == Summary::BaselineFailed)
            || self
                .outcomes
                .iter()
                .any(|o| matches!(o.scenario, Scenario::Baseline) && o.summary != Summary::Success)
        {
            ExitCode::BaselineFailed
        } else if self.timeout > 0 {
            ExitCode::Timeout
        } else if self.missed > 0 {
            ExitCode::Missed
        } else {
            ExitCode::Success
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phase(phase: Phase, status: ProcessStatus) -> PhaseResult {
        PhaseResult {
            phase,
            duration: 1.0,
            process_status: status,
            argv: Vec::new(),
        }
    }

    #[test]
    fn summarizes_phases() {
        use ProcessStatus::*;
        let build = |s| phase(Phase::Build, s);
        let test = |s| phase(Phase::Test, s);
        assert_eq!(
            Summary::from_phases(false, &[build(Failure(101))], true),
            Summary::Unviable
        );
        assert_eq!(
            Summary::from_phases(false, &[build(Timeout)], true),
            Summary::Timeout
        );
        assert_eq!(
            Summary::from_phases(false, &[build(Success), test(Failure(100))], true),
            Summary::CaughtMutant
        );
        assert_eq!(
            Summary::from_phases(false, &[build(Success), test(Signalled)], true),
            Summary::CaughtMutant
        );
        assert_eq!(
            Summary::from_phases(false, &[build(Success), test(Success)], true),
            Summary::MissedMutant
        );
        assert_eq!(
            Summary::from_phases(false, &[build(Success)], false),
            Summary::Success
        );
        assert_eq!(
            Summary::from_phases(true, &[build(Success), test(Failure(1))], true),
            Summary::BaselineFailed
        );
        assert_eq!(
            Summary::from_phases(true, &[build(Success), test(Success)], true),
            Summary::Success
        );
    }

    #[test]
    fn serializes_like_cargo_mutants() {
        let result = phase(Phase::Build, ProcessStatus::Failure(101));
        assert_eq!(
            serde_json::to_string(&result.process_status).unwrap(),
            r#"{"Failure":101}"#
        );
        assert_eq!(
            serde_json::to_string(&ProcessStatus::Success).unwrap(),
            r#""Success""#
        );
        assert_eq!(
            serde_json::to_string(&Scenario::Baseline).unwrap(),
            r#""Baseline""#
        );
        assert_eq!(
            serde_json::to_string(&Summary::CaughtMutant).unwrap(),
            r#""CaughtMutant""#
        );
    }

    #[test]
    fn exit_codes_rank_timeouts_over_misses() {
        let mut lab = LabOutcome::new(2);
        assert_eq!(lab.exit_code(), ExitCode::Success);
        lab.missed = 1;
        assert_eq!(lab.exit_code(), ExitCode::Missed);
        lab.timeout = 1;
        assert_eq!(lab.exit_code(), ExitCode::Timeout);
    }
}
