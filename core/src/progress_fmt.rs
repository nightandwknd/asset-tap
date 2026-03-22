//! Shared progress formatting for CLI and GUI.
//!
//! Provides consistent progress message formatting across different interfaces.

use crate::types::{Progress, Stage};

/// Format a progress update as a human-readable message.
///
/// Returns a tuple of (icon, message) where icon is a unicode emoji
/// suitable for CLI or a hint for GUI icon selection.
pub fn format_progress(progress: &Progress) -> ProgressDisplay {
    match progress {
        Progress::Started { stage, .. } => ProgressDisplay {
            icon: stage_icon(stage),
            message: format!("{}...", stage),
            level: DisplayLevel::Info,
        },
        Progress::Queued {
            stage, position, ..
        } => ProgressDisplay {
            icon: "⏳",
            message: format!("{}: Queued (position {})", stage, position),
            level: DisplayLevel::Info,
        },
        Progress::Processing { stage, message, .. } => {
            if let Some(msg) = message {
                ProgressDisplay {
                    icon: "🔄",
                    message: format!("{}: {}", stage, msg),
                    level: DisplayLevel::Info,
                }
            } else {
                ProgressDisplay {
                    icon: "🔄",
                    message: format!("{}: Processing...", stage),
                    level: DisplayLevel::Info,
                }
            }
        }
        Progress::Completed { stage, .. } => ProgressDisplay {
            icon: "✅",
            message: format!("{} complete", stage),
            level: DisplayLevel::Success,
        },
        Progress::Failed { stage, error, .. } => ProgressDisplay {
            icon: "❌",
            message: format!("{} failed: {}", stage, error),
            level: DisplayLevel::Error,
        },
        Progress::Downloading {
            stage,
            bytes_downloaded,
            total_bytes,
            ..
        } => {
            let msg = if let Some(total) = total_bytes {
                let pct = (*bytes_downloaded as f64 / *total as f64) * 100.0;
                format!("{}: Downloading {:.1}%", stage, pct)
            } else {
                format!("{}: Downloading {} bytes", stage, bytes_downloaded)
            };
            ProgressDisplay {
                icon: "⬇️",
                message: msg,
                level: DisplayLevel::Info,
            }
        }
        Progress::Log { stage, message, .. } => ProgressDisplay {
            icon: "📝",
            message: format!("{}: {}", stage, message),
            level: DisplayLevel::Debug,
        },
        Progress::Retrying {
            stage,
            attempt,
            max_attempts,
            delay_secs,
            reason,
            ..
        } => ProgressDisplay {
            icon: "🔁",
            message: format!(
                "{}: {} Retrying ({}/{}) in {}s...",
                stage, reason, attempt, max_attempts, delay_secs
            ),
            level: DisplayLevel::Warning,
        },
        Progress::AwaitingApproval { stage, .. } => ProgressDisplay {
            icon: "👁️",
            message: format!("{}: Awaiting approval for generated image", stage),
            level: DisplayLevel::Info,
        },
    }
}

/// A formatted progress display.
#[derive(Debug, Clone)]
pub struct ProgressDisplay {
    /// Unicode emoji icon for the progress type.
    pub icon: &'static str,
    /// Human-readable message.
    pub message: String,
    /// Display level/severity.
    pub level: DisplayLevel,
}

impl ProgressDisplay {
    /// Format for CLI output with emoji prefix.
    pub fn cli_format(&self) -> String {
        format!("{} {}", self.icon, self.message)
    }

    /// Get the stage from the original progress (if needed for styling).
    pub fn stage_from(progress: &Progress) -> Option<Stage> {
        match progress {
            Progress::Started { stage, .. }
            | Progress::Queued { stage, .. }
            | Progress::Processing { stage, .. }
            | Progress::Completed { stage, .. }
            | Progress::Failed { stage, .. }
            | Progress::Downloading { stage, .. }
            | Progress::Log { stage, .. }
            | Progress::Retrying { stage, .. }
            | Progress::AwaitingApproval { stage, .. } => Some(*stage),
        }
    }
}

/// Display level for styling purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayLevel {
    /// Debug/log message (dim/gray).
    Debug,
    /// Normal informational message.
    Info,
    /// Success message (green).
    Success,
    /// Warning message (yellow).
    Warning,
    /// Error message (red).
    Error,
}

/// Get the emoji icon for a pipeline stage.
pub fn stage_icon(stage: &Stage) -> &'static str {
    match stage {
        Stage::ImageGeneration => "🎨",
        Stage::Model3DGeneration => "🧊",
        Stage::FbxConversion => "🔄",
        Stage::Download => "⬇️",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_started() {
        let progress = Progress::Started {
            stage: Stage::ImageGeneration,
        };
        let display = format_progress(&progress);
        assert_eq!(display.icon, "🎨");
        assert!(display.message.contains("Image Generation"));
        assert_eq!(display.level, DisplayLevel::Info);
    }

    #[test]
    fn test_format_completed() {
        let progress = Progress::Completed {
            stage: Stage::Model3DGeneration,
        };
        let display = format_progress(&progress);
        assert_eq!(display.icon, "✅");
        assert!(display.message.contains("3D Model Generation"));
        assert_eq!(display.level, DisplayLevel::Success);
    }

    #[test]
    fn test_format_failed() {
        let progress = Progress::Failed {
            stage: Stage::Model3DGeneration,
            error: "Model too complex".to_string(),
        };
        let display = format_progress(&progress);
        assert_eq!(display.icon, "❌");
        assert!(display.message.contains("failed"));
        assert!(display.message.contains("Model too complex"));
        assert_eq!(display.level, DisplayLevel::Error);
    }

    #[test]
    fn test_cli_format() {
        let progress = Progress::Completed {
            stage: Stage::Download,
        };
        let display = format_progress(&progress);
        let cli_output = display.cli_format();
        assert!(cli_output.starts_with("✅"));
        assert!(cli_output.contains("Download complete"));
    }

    #[test]
    fn test_format_queued() {
        let progress = Progress::Queued {
            stage: Stage::Model3DGeneration,
            position: 5,
        };
        let display = format_progress(&progress);
        assert_eq!(display.icon, "⏳");
        assert!(display.message.contains("Queued"));
        assert!(display.message.contains("position 5"));
        assert_eq!(display.level, DisplayLevel::Info);
    }

    #[test]
    fn test_format_processing_with_message() {
        let progress = Progress::Processing {
            stage: Stage::ImageGeneration,
            message: Some("Generating...".to_string()),
        };
        let display = format_progress(&progress);
        assert_eq!(display.icon, "🔄");
        assert!(display.message.contains("Generating..."));
        assert_eq!(display.level, DisplayLevel::Info);
    }

    #[test]
    fn test_format_processing_without_message() {
        let progress = Progress::Processing {
            stage: Stage::ImageGeneration,
            message: None,
        };
        let display = format_progress(&progress);
        assert_eq!(display.icon, "🔄");
        assert!(display.message.contains("Processing..."));
        assert_eq!(display.level, DisplayLevel::Info);
    }

    #[test]
    fn test_format_downloading_with_total() {
        let progress = Progress::Downloading {
            stage: Stage::Download,
            bytes_downloaded: 50,
            total_bytes: Some(100),
        };
        let display = format_progress(&progress);
        assert_eq!(display.icon, "⬇️");
        assert!(display.message.contains("50.0%"));
        assert_eq!(display.level, DisplayLevel::Info);
    }

    #[test]
    fn test_format_downloading_without_total() {
        let progress = Progress::Downloading {
            stage: Stage::Download,
            bytes_downloaded: 1024,
            total_bytes: None,
        };
        let display = format_progress(&progress);
        assert!(display.message.contains("1024 bytes"));
    }

    #[test]
    fn test_format_log() {
        let progress = Progress::Log {
            stage: Stage::Model3DGeneration,
            message: "Step 5/10".to_string(),
        };
        let display = format_progress(&progress);
        assert_eq!(display.icon, "📝");
        assert!(display.message.contains("Step 5/10"));
        assert_eq!(display.level, DisplayLevel::Debug);
    }

    #[test]
    fn test_format_retrying() {
        let progress = Progress::Retrying {
            stage: Stage::ImageGeneration,
            attempt: 2,
            max_attempts: 3,
            delay_secs: 10,
            reason: "Rate limited".to_string(),
        };
        let display = format_progress(&progress);
        assert_eq!(display.icon, "🔁");
        assert!(display.message.contains("Rate limited"));
        assert!(display.message.contains("2/3"));
        assert!(display.message.contains("10s"));
        assert_eq!(display.level, DisplayLevel::Warning);
    }

    #[test]
    fn test_stage_icons() {
        assert_eq!(stage_icon(&Stage::ImageGeneration), "🎨");
        assert_eq!(stage_icon(&Stage::Model3DGeneration), "🧊");
        assert_eq!(stage_icon(&Stage::FbxConversion), "🔄");
        assert_eq!(stage_icon(&Stage::Download), "⬇️");
    }

    #[test]
    fn test_stage_from_progress() {
        let test_cases = vec![
            Progress::Started {
                stage: Stage::ImageGeneration,
            },
            Progress::Queued {
                stage: Stage::ImageGeneration,
                position: 1,
            },
            Progress::Processing {
                stage: Stage::Model3DGeneration,
                message: None,
            },
            Progress::Completed {
                stage: Stage::FbxConversion,
            },
            Progress::Failed {
                stage: Stage::FbxConversion,
                error: "err".to_string(),
            },
            Progress::Downloading {
                stage: Stage::Download,
                bytes_downloaded: 0,
                total_bytes: None,
            },
            Progress::Log {
                stage: Stage::ImageGeneration,
                message: "log".to_string(),
            },
            Progress::Retrying {
                stage: Stage::Model3DGeneration,
                attempt: 1,
                max_attempts: 3,
                delay_secs: 5,
                reason: "test".to_string(),
            },
        ];

        for progress in test_cases {
            assert!(ProgressDisplay::stage_from(&progress).is_some());
        }
    }

    #[test]
    fn test_display_level_equality() {
        assert_eq!(DisplayLevel::Debug, DisplayLevel::Debug);
        assert_eq!(DisplayLevel::Info, DisplayLevel::Info);
        assert_eq!(DisplayLevel::Success, DisplayLevel::Success);
        assert_eq!(DisplayLevel::Warning, DisplayLevel::Warning);
        assert_eq!(DisplayLevel::Error, DisplayLevel::Error);
        assert_ne!(DisplayLevel::Debug, DisplayLevel::Error);
    }
}
