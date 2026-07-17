//! A lasting record of everything said through the bot.
//!
//! Only the text is kept. The audio of an utterance is transcribed, spoken back, and
//! dropped — it is never written to disk.
//!
//! The file is JSON Lines: one self-contained JSON object per line, appended as each
//! utterance is heard. That shape is chosen deliberately over one big JSON array — an
//! append is a write of one line rather than a rewrite of the whole history, and a file
//! truncated by a crash loses its last line rather than becoming unparseable.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const TRANSCRIPTS_FILE_NAME: &str = "butter-tts.transcripts.jsonl";

/// Beyond this the history is trimmed, oldest first. A record of everything ever said is
/// the point, but not at the cost of a file that grows without limit.
const MAX_TRANSCRIPTS: usize = 10_000;

/// Rewriting the file on every line would make each utterance cost a full history
/// rewrite, so the trim happens in batches once it has drifted far enough over.
const TRIM_SLACK: usize = 500;

/// One thing the user said, as text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transcript {
    /// Milliseconds since the Unix epoch.
    pub timestamp_ms: u64,
    /// What the transcription heard.
    pub text: String,
    /// The voice it was spoken back in, so the history says how it sounded.
    pub voice: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TranscriptError {
    #[error("could not work out where the app is running from: {0}")]
    ExeLocation(#[source] std::io::Error),

    #[error("the app is running from a filesystem root, so there is nowhere to keep its history")]
    NoExeDirectory,

    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not encode a transcript: {0}")]
    Encode(#[source] serde_json::Error),
}

/// Where the history lives: alongside the exe, like the settings.
pub fn transcripts_path() -> Result<PathBuf, TranscriptError> {
    let exe_path = std::env::current_exe().map_err(TranscriptError::ExeLocation)?;

    let Some(exe_directory) = exe_path.parent() else {
        return Err(TranscriptError::NoExeDirectory);
    };

    Ok(exe_directory.join(TRANSCRIPTS_FILE_NAME))
}

/// Adds one line to the history.
pub fn append(transcript: &Transcript) -> Result<(), TranscriptError> {
    let path = transcripts_path()?;
    append_to(&path, transcript)
}

/// Everything said so far, oldest first.
pub fn load_all() -> Result<Vec<Transcript>, TranscriptError> {
    let path = transcripts_path()?;
    read_from(&path)
}

/// Forgets the whole history.
pub fn clear() -> Result<(), TranscriptError> {
    let path = transcripts_path()?;

    if !path.exists() {
        return Ok(());
    }

    std::fs::remove_file(&path).map_err(|source| TranscriptError::Write { path, source })
}

fn append_to(path: &Path, transcript: &Transcript) -> Result<(), TranscriptError> {
    let line = serde_json::to_string(transcript).map_err(TranscriptError::Encode)?;

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| TranscriptError::Write {
            path: path.to_path_buf(),
            source,
        })?;

    writeln!(file, "{line}").map_err(|source| TranscriptError::Write {
        path: path.to_path_buf(),
        source,
    })?;

    trim_if_overgrown(path)
}

/// Reads the history, skipping any line that will not parse.
///
/// A corrupt line is one lost utterance; refusing to open the history over it would lose
/// all of them.
fn read_from(path: &Path) -> Result<Vec<Transcript>, TranscriptError> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let contents = std::fs::read_to_string(path).map_err(|source| TranscriptError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    Ok(parse_lines(&contents))
}

fn parse_lines(contents: &str) -> Vec<Transcript> {
    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<Transcript>(line).ok())
        .collect()
}

/// Drops the oldest lines once the file has grown past the cap plus its slack.
fn trim_if_overgrown(path: &Path) -> Result<(), TranscriptError> {
    let transcripts = read_from(path)?;

    if transcripts.len() <= MAX_TRANSCRIPTS + TRIM_SLACK {
        return Ok(());
    }

    let keep_from = transcripts.len() - MAX_TRANSCRIPTS;
    write_all(path, &transcripts[keep_from..])
}

fn write_all(path: &Path, transcripts: &[Transcript]) -> Result<(), TranscriptError> {
    let mut contents = String::new();

    for transcript in transcripts {
        let line = serde_json::to_string(transcript).map_err(TranscriptError::Encode)?;
        contents.push_str(&line);
        contents.push('\n');
    }

    std::fs::write(path, contents).map_err(|source| TranscriptError::Write {
        path: path.to_path_buf(),
        source,
    })
}

/// Builds a transcript stamped with the current time.
pub fn record(text: &str, voice: &str) -> Transcript {
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since_epoch| since_epoch.as_millis() as u64)
        .unwrap_or(0);

    Transcript {
        timestamp_ms,
        text: text.to_string(),
        voice: voice.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(test_name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("butter-tts-{test_name}.transcripts.jsonl"));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn sample(text: &str) -> Transcript {
        Transcript {
            timestamp_ms: 1_700_000_000_000,
            text: text.to_string(),
            voice: "marin".to_string(),
        }
    }

    #[test]
    fn a_missing_history_reads_as_empty_rather_than_failing() {
        let path = temp_path("missing");

        assert_eq!(read_from(&path).expect("should load"), Vec::new());
    }

    #[test]
    fn appended_lines_come_back_in_the_order_they_were_said() {
        let path = temp_path("order");

        append_to(&path, &sample("first thing")).expect("append should work");
        append_to(&path, &sample("second thing")).expect("append should work");

        let loaded = read_from(&path).expect("should load");

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].text, "first thing");
        assert_eq!(loaded[1].text, "second thing");
    }

    #[test]
    fn each_utterance_is_one_line() {
        let path = temp_path("lines");

        append_to(&path, &sample("hello")).expect("append should work");
        append_to(&path, &sample("goodbye")).expect("append should work");

        let contents = std::fs::read_to_string(&path).expect("file should exist");

        assert_eq!(contents.lines().count(), 2);
    }

    #[test]
    fn a_corrupt_line_costs_one_utterance_not_the_whole_history() {
        let path = temp_path("corrupt");
        let good_line = serde_json::to_string(&sample("survivor")).expect("should encode");
        std::fs::write(&path, format!("{{not json at all\n{good_line}\n")).expect("write");

        let loaded = read_from(&path).expect("a corrupt line should not fail the read");

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].text, "survivor");
    }

    #[test]
    fn blank_lines_are_ignored() {
        let good_line = serde_json::to_string(&sample("hi")).expect("should encode");

        let parsed = parse_lines(&format!("\n{good_line}\n\n"));

        assert_eq!(parsed.len(), 1);
    }

    #[test]
    fn text_with_newlines_and_quotes_survives_the_round_trip() {
        let path = temp_path("awkward");
        // JSON escapes these, so they cannot break the one-object-per-line shape.
        let awkward = sample("she said \"hi\"\nthen left");

        append_to(&path, &awkward).expect("append should work");
        let loaded = read_from(&path).expect("should load");

        assert_eq!(loaded, vec![awkward]);
    }

    #[test]
    fn the_history_is_trimmed_once_it_grows_past_the_cap() {
        let path = temp_path("trim");
        let overgrown: Vec<Transcript> = (0..MAX_TRANSCRIPTS + TRIM_SLACK + 1)
            .map(|index| sample(&format!("line {index}")))
            .collect();
        write_all(&path, &overgrown).expect("write should work");

        trim_if_overgrown(&path).expect("trim should work");
        let loaded = read_from(&path).expect("should load");

        assert_eq!(loaded.len(), MAX_TRANSCRIPTS);
        // The oldest go first; what was said most recently is what is kept.
        assert_eq!(loaded.last().expect("not empty").text, "line 10500");
    }

    #[test]
    fn a_history_under_the_cap_is_left_alone() {
        let path = temp_path("no-trim");
        let modest: Vec<Transcript> = (0..10).map(|index| sample(&format!("{index}"))).collect();
        write_all(&path, &modest).expect("write should work");

        trim_if_overgrown(&path).expect("trim should work");

        assert_eq!(read_from(&path).expect("should load").len(), 10);
    }

    #[test]
    fn a_recorded_transcript_is_stamped_with_a_real_time() {
        let transcript = record("hello", "cedar");

        assert_eq!(transcript.text, "hello");
        assert_eq!(transcript.voice, "cedar");
        // Any plausible clock is past 2020; zero would mean the fallback fired.
        assert!(transcript.timestamp_ms > 1_577_836_800_000);
    }
}
