use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StepStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanItem {
    pub(crate) step: String,
    pub(crate) status: StepStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanUpdate {
    pub(crate) explanation: Option<String>,
    pub(crate) plan: Vec<PlanItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileChange {
    Add {
        content: String,
    },
    Delete {
        content: String,
    },
    Update {
        unified_diff: String,
        move_path: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub total_tokens: i64,
}

impl TokenUsage {
    pub fn is_zero(&self) -> bool {
        self.total_tokens == 0
    }

    fn cached_input(&self) -> i64 {
        self.cached_input_tokens.max(0)
    }

    fn non_cached_input(&self) -> i64 {
        (self.input_tokens - self.cached_input()).max(0)
    }

    fn blended_total(&self) -> i64 {
        (self.non_cached_input() + self.output_tokens.max(0)).max(0)
    }
}

impl fmt::Display for TokenUsage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cached = if self.cached_input() > 0 {
            format!(" (+ {} cached)", format_number(self.cached_input()))
        } else {
            String::new()
        };
        let reasoning = if self.reasoning_output_tokens > 0 {
            format!(
                " (reasoning {})",
                format_number(self.reasoning_output_tokens)
            )
        } else {
            String::new()
        };
        write!(
            f,
            "Token usage: total={} input={}{} output={}{}",
            format_number(self.blended_total()),
            format_number(self.non_cached_input()),
            cached,
            format_number(self.output_tokens),
            reasoning,
        )
    }
}

fn format_number(value: i64) -> String {
    let digits = value.abs().to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(digit);
    }
    if value < 0 {
        formatted.insert(0, '-');
    }
    formatted
}

pub(crate) fn format_si_suffix(value: i64) -> String {
    let value = value.max(0);
    if value < 1_000 {
        return value.to_string();
    }

    for (scale, suffix) in [(1_000_i64, "K"), (1_000_000, "M"), (1_000_000_000, "G")] {
        let scaled = value as f64 / scale as f64;
        let rounded = if scaled < 10.0 {
            format!("{scaled:.2}")
        } else if scaled < 100.0 {
            format!("{scaled:.1}")
        } else if scaled < 999.5 {
            format!("{scaled:.0}")
        } else {
            continue;
        };
        return format!("{rounded}{suffix}");
    }

    format!("{}G", format_number((value as f64 / 1e9).round() as i64))
}

pub(crate) fn format_elapsed_seconds(seconds: i64) -> String {
    let seconds = seconds.max(0);
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m {}s", seconds / 60, seconds % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_number_and_duration_formatting() {
        assert_eq!(format_si_suffix(999), "999");
        assert_eq!(format_si_suffix(1_200), "1.20K");
        assert_eq!(format_si_suffix(123_456_789), "123M");
        assert_eq!(format_elapsed_seconds(73), "1m 13s");
    }
}
