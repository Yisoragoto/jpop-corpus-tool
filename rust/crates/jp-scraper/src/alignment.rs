//! Extension boundary only: no model, audio upload or automatic adoption.
//! Generated timings remain a proposal until a user validates them; callers must
//! not insert them as `lrc`/`manual` or label them native source timestamps.
use crate::timed_lyrics::TimedLyrics;
use anyhow::Result;
use std::path::Path;

pub struct AlignmentProposal {
    pub engine: String,
    pub engine_version: String,
    pub lyrics: TimedLyrics,
    pub diagnostics: Vec<String>,
}

/// Implementations must work on audio the user has chosen and can process.
/// They belong on a background worker, outside the corpus transaction.
pub trait AudioAligner: Send + Sync {
    fn propose(&self, audio: &Path, original_text: &str) -> Result<AlignmentProposal>;
}
