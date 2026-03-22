//! Validation constants for bundle metadata and user input.
//!
//! These constants define the limits and constraints for various fields
//! in the bundle metadata structure.

/// Maximum supported bundle format version
pub const MAX_VERSION: u32 = 100;

/// Maximum length of bundle name
pub const MAX_NAME_LENGTH: usize = 500;

/// Maximum length of notes field
pub const MAX_NOTES_LENGTH: usize = 10_000;

/// Maximum number of tags allowed
pub const MAX_TAGS: usize = 100;

/// Maximum length of a single tag
pub const MAX_TAG_LENGTH: usize = 100;

/// One day in seconds (used for time calculations)
pub const ONE_DAY_SECS: i64 = 86_400;

/// One week in milliseconds (maximum allowed generation duration)
pub const ONE_WEEK_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// Maximum generation duration in milliseconds (1 week)
pub const MAX_DURATION_MS: u64 = ONE_WEEK_MS;

/// Future timestamp tolerance in seconds (1 day)
/// Allows for timestamps slightly in the future to account for clock skew
pub const FUTURE_TOLERANCE_SECS: i64 = ONE_DAY_SECS;

/// Maximum number of history records to retain
pub const MAX_HISTORY_RECORDS: usize = 1000;

/// Maximum combined prompt length (description + template expansion).
/// Most image models use CLIP tokenizers (77-256 tokens ≈ 300-1000 useful chars).
/// 10,000 chars is generous while preventing abuse.
pub const MAX_PROMPT_LENGTH: usize = 10_000;
