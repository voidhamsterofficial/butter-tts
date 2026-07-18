//! A lasting record of everything said through the bot, kept in the database's
//! `transcripts` table.
//!
//! Only the text is kept. The audio of an utterance is transcribed, spoken back, and
//! dropped — it is never written to disk.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::StoreError;

/// Beyond this the history is trimmed, oldest first. A record of everything ever said is
/// the point, but not at the cost of a database that grows without limit.
const MAX_TRANSCRIPTS: i64 = 10_000;

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

/// Adds one entry to the history.
pub fn append(transcript: &Transcript) -> Result<(), StoreError> {
    let connection = super::open()?;
    append_to(&connection, transcript)
}

/// Everything said so far, oldest first.
pub fn load_all() -> Result<Vec<Transcript>, StoreError> {
    let connection = super::open()?;
    load_all_from(&connection)
}

/// Forgets the whole history.
pub fn clear() -> Result<(), StoreError> {
    let connection = super::open()?;
    connection
        .execute("DELETE FROM transcripts", [])
        .map_err(StoreError::Query)?;
    Ok(())
}

fn append_to(connection: &Connection, transcript: &Transcript) -> Result<(), StoreError> {
    connection
        .execute(
            "INSERT INTO transcripts (timestamp_ms, text, voice) VALUES (?1, ?2, ?3)",
            params![
                transcript.timestamp_ms as i64,
                transcript.text,
                transcript.voice
            ],
        )
        .map_err(StoreError::Query)?;

    trim_if_overgrown(connection)
}

fn load_all_from(connection: &Connection) -> Result<Vec<Transcript>, StoreError> {
    let mut statement = connection
        .prepare("SELECT timestamp_ms, text, voice FROM transcripts ORDER BY id ASC")
        .map_err(StoreError::Query)?;

    let rows = statement
        .query_map([], |row| {
            let timestamp_ms: i64 = row.get(0)?;
            Ok(Transcript {
                timestamp_ms: timestamp_ms as u64,
                text: row.get(1)?,
                voice: row.get(2)?,
            })
        })
        .map_err(StoreError::Query)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::Query)
}

/// Drops the oldest rows once the table has grown past the cap. Cheap enough to run on
/// every insert — it is one indexed delete, not a rewrite of everything kept.
fn trim_if_overgrown(connection: &Connection) -> Result<(), StoreError> {
    connection
        .execute(
            "DELETE FROM transcripts WHERE id NOT IN (
                SELECT id FROM transcripts ORDER BY id DESC LIMIT ?1
             )",
            params![MAX_TRANSCRIPTS],
        )
        .map_err(StoreError::Query)?;

    Ok(())
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

    fn sample(text: &str) -> Transcript {
        Transcript {
            timestamp_ms: 1_700_000_000_000,
            text: text.to_string(),
            voice: "marin".to_string(),
        }
    }

    fn test_db() -> Connection {
        let connection = Connection::open_in_memory().expect("in-memory db should open");
        connection
            .execute_batch(super::super::SCHEMA_SQL)
            .expect("schema should apply");
        connection
    }

    #[test]
    fn an_empty_history_reads_as_empty_rather_than_failing() {
        let connection = test_db();

        assert_eq!(load_all_from(&connection).expect("should load"), Vec::new());
    }

    #[test]
    fn appended_entries_come_back_in_the_order_they_were_said() {
        let connection = test_db();

        append_to(&connection, &sample("first thing")).expect("append should work");
        append_to(&connection, &sample("second thing")).expect("append should work");

        let loaded = load_all_from(&connection).expect("should load");

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].text, "first thing");
        assert_eq!(loaded[1].text, "second thing");
    }

    #[test]
    fn text_with_newlines_and_quotes_survives_the_round_trip() {
        let connection = test_db();
        let awkward = sample("she said \"hi\"\nthen left");

        append_to(&connection, &awkward).expect("append should work");
        let loaded = load_all_from(&connection).expect("should load");

        assert_eq!(loaded, vec![awkward]);
    }

    #[test]
    fn the_history_is_trimmed_once_it_grows_past_the_cap() {
        let connection = test_db();

        for index in 0..(MAX_TRANSCRIPTS + 50) {
            append_to(&connection, &sample(&format!("line {index}"))).expect("append should work");
        }

        let loaded = load_all_from(&connection).expect("should load");

        assert_eq!(loaded.len(), MAX_TRANSCRIPTS as usize);
        // The oldest go first; what was said most recently is what is kept.
        assert_eq!(loaded.last().expect("not empty").text, "line 10049");
        assert_eq!(loaded.first().expect("not empty").text, "line 50");
    }

    #[test]
    fn a_history_under_the_cap_is_left_alone() {
        let connection = test_db();

        for index in 0..10 {
            append_to(&connection, &sample(&format!("{index}"))).expect("append should work");
        }

        assert_eq!(load_all_from(&connection).expect("should load").len(), 10);
    }

    #[test]
    fn clearing_removes_everything() {
        let connection = test_db();
        append_to(&connection, &sample("hello")).expect("append should work");

        connection
            .execute("DELETE FROM transcripts", [])
            .expect("clear should work");

        assert_eq!(load_all_from(&connection).expect("should load"), Vec::new());
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
