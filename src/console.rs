//! Progress lines and the final summary, in cargo-mutants' voice.

use crate::outcome::{LabOutcome, Phase, ScenarioOutcome, Summary};

#[derive(Clone, Copy, Debug)]
pub struct Console {
    pub show_caught: bool,
    pub show_unviable: bool,
    pub no_times: bool,
}

impl Console {
    pub fn found(&self, count: usize) {
        println!("Found {count} mutants to test");
    }

    /// One line per scenario worth mentioning: everything for the baseline,
    /// misses and timeouts for mutants, the rest on request.
    pub fn scenario(&self, outcome: &ScenarioOutcome) {
        let show = match outcome.summary {
            Summary::CaughtMutant => self.show_caught,
            Summary::Unviable => self.show_unviable,
            _ => true,
        };
        if show {
            println!(
                "{:<8} {}{}",
                outcome.summary.label(),
                outcome.name(),
                self.times(outcome)
            );
        }
    }

    /// ` in 1.2s build + 0.3s test`, for the phases that ran.
    fn times(&self, outcome: &ScenarioOutcome) -> String {
        if self.no_times {
            return String::new();
        }
        let phases: Vec<String> = [Phase::Build, Phase::Test]
            .into_iter()
            .filter_map(|phase| {
                outcome
                    .duration(phase)
                    .map(|secs| format!("{} {}", Self::duration(secs), phase_name(phase)))
            })
            .collect();
        if phases.is_empty() {
            String::new()
        } else {
            format!(" in {}", phases.join(" + "))
        }
    }

    pub fn summary(&self, lab: &LabOutcome, elapsed: f64) {
        let mut parts = Vec::new();
        for (count, label) in [
            (lab.caught, "caught"),
            (lab.missed, "missed"),
            (lab.unviable, "unviable"),
            (lab.timeout, "timeout"),
            (lab.success, "succeeded"),
        ] {
            if count > 0 {
                parts.push(format!("{count} {label}"));
            }
        }
        let when = if self.no_times {
            String::new()
        } else {
            format!(" in {}", Self::duration(elapsed))
        };
        println!(
            "{} mutants tested{when}: {}",
            lab.total_mutants,
            parts.join(", ")
        );
    }

    /// `12.3s`, `1m 2s`, `1h 4m`.
    pub fn duration(secs: f64) -> String {
        let whole = secs as u64;
        match whole {
            0..=59 => format!("{secs:.1}s"),
            60..=3599 => format!("{}m {}s", whole / 60, whole % 60),
            _ => format!("{}h {}m", whole / 3600, (whole % 3600) / 60),
        }
    }
}

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Build => "build",
        Phase::Test => "test",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_durations() {
        assert_eq!(Console::duration(0.25), "0.2s");
        assert_eq!(Console::duration(12.34), "12.3s");
        assert_eq!(Console::duration(62.0), "1m 2s");
        assert_eq!(Console::duration(3840.0), "1h 4m");
    }
}
