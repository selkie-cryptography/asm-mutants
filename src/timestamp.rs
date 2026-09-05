//! ISO 8601 UTC timestamps for the output files, without a date crate.

use std::time::{SystemTime, UNIX_EPOCH};

/// A UTC instant with microsecond precision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timestamp {
    seconds: u64,
    micros: u32,
}

impl Timestamp {
    pub fn now() -> Self {
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Self {
            seconds: since_epoch.as_secs(),
            micros: since_epoch.subsec_micros(),
        }
    }

    /// Civil date from days since the epoch (Howard Hinnant's algorithm).
    fn civil(days: u64) -> (u64, u64, u64) {
        let z = days + 719_468;
        let era = z / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        (if m <= 2 { y + 1 } else { y }, m, d)
    }

    /// `YYYY-MM-DDTHH:MM:SS.ffffffZ`.
    pub fn iso8601(&self) -> String {
        let (year, month, day) = Self::civil(self.seconds / 86_400);
        let secs = self.seconds % 86_400;
        format!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:06}Z",
            secs / 3600,
            (secs % 3600) / 60,
            secs % 60,
            self.micros
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_known_instants() {
        let epoch = Timestamp {
            seconds: 0,
            micros: 0,
        };
        assert_eq!(epoch.iso8601(), "1970-01-01T00:00:00.000000Z");
        // 2026-07-30T16:49:13Z, the start_time of a cargo-mutants run.
        let run = Timestamp {
            seconds: 1_785_430_153,
            micros: 538_041,
        };
        assert_eq!(run.iso8601(), "2026-07-30T16:49:13.538041Z");
    }
}
