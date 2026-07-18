//! Everything the app keeps: one SQLite database holding both the settings and the
//! history, encrypted where it matters. See [`crypto`] for the encryption and
//! [`settings`] / [`transcripts`] for what is actually stored.
//!
//! # Where the database lives
//!
//! The app supports two locations, and asks the user to pick one the first time it
//! runs (see [`needs_setup`] / [`set_up`]):
//!
//! - **Portable**: next to the running executable. Copy that folder to a USB stick and
//!   it works the same on the next machine, leaving nothing behind on this one. This is
//!   the only sane choice for the portable Windows build; on an installed app it works
//!   too, but the folder disappears when the app is reinstalled or updated.
//! - **Default**: the OS's own per-user application data folder, which survives
//!   reinstalls and updates.
//!
//! Every lookup after that first run checks the portable location first, then the
//! default one — whichever one actually has a database wins, so nothing needs to be
//! remembered anywhere beyond the two obvious places to look.

pub mod crypto;
pub mod settings;
pub mod transcripts;

use std::path::PathBuf;

use rusqlite::Connection;

const DATABASE_FILE_NAME: &str = "butter-tts.db";

const SCHEMA_SQL: &str = "
    CREATE TABLE IF NOT EXISTS settings (
        id INTEGER PRIMARY KEY CHECK (id = 0),
        openai_api_key BLOB NOT NULL,
        discord_bot_token BLOB NOT NULL,
        microphone_name TEXT NOT NULL DEFAULT '',
        tts_voice TEXT NOT NULL DEFAULT '',
        noise_suppression INTEGER NOT NULL DEFAULT 1,
        speech_threshold INTEGER NOT NULL DEFAULT 0,
        trailing_silence_ms INTEGER NOT NULL DEFAULT 0,
        min_utterance_ms INTEGER NOT NULL DEFAULT 0,
        max_utterance_ms INTEGER NOT NULL DEFAULT 0
    );

    CREATE TABLE IF NOT EXISTS transcripts (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        timestamp_ms INTEGER NOT NULL,
        text TEXT NOT NULL,
        voice TEXT NOT NULL
    );
";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("could not work out where the app is running from: {0}")]
    ExeLocation(#[source] std::io::Error),

    #[error("the app is running from a filesystem root, so there is nowhere to keep its database")]
    NoExeDirectory,

    #[error("could not find the system's application data folder")]
    NoDefaultDirectory,

    #[error("could not create {path}: {source}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not open the database at {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },

    #[error("a database query failed: {0}")]
    Query(#[source] rusqlite::Error),

    #[error("no database has been set up yet")]
    NotSetUp,
}

/// The two places [`set_up`] will put the database. See the module docs for what each
/// one means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Location {
    Portable,
    Default,
}

/// True until the user has picked a location and something has been set up there. The
/// frontend checks this on startup to decide whether to show the location picker before
/// anything else.
pub fn needs_setup() -> bool {
    find_existing().is_none()
}

/// Creates the database at the chosen location (if nothing is there yet) and returns
/// its path.
pub fn set_up(location: Location) -> Result<PathBuf, StoreError> {
    let directory = match location {
        Location::Portable => portable_dir()?,
        Location::Default => default_dir()?,
    };

    std::fs::create_dir_all(&directory).map_err(|source| StoreError::CreateDirectory {
        path: directory.clone(),
        source,
    })?;

    let path = directory.join(DATABASE_FILE_NAME);
    open_at(&path)?; // Creates the file and the schema as a side effect.

    Ok(path)
}

/// Opens the database, wherever [`set_up`] put it.
pub(crate) fn open() -> Result<Connection, StoreError> {
    let path = find_existing().ok_or(StoreError::NotSetUp)?;
    open_at(&path)
}

/// The database's own path, for the settings and history pages to show the user where
/// their data lives.
pub fn database_path() -> Result<PathBuf, StoreError> {
    find_existing().ok_or(StoreError::NotSetUp)
}

fn find_existing() -> Option<PathBuf> {
    if let Ok(directory) = portable_dir() {
        let candidate = directory.join(DATABASE_FILE_NAME);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    if let Ok(directory) = default_dir() {
        let candidate = directory.join(DATABASE_FILE_NAME);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

fn open_at(path: &std::path::Path) -> Result<Connection, StoreError> {
    let connection = Connection::open(path).map_err(|source| StoreError::Open {
        path: path.to_path_buf(),
        source,
    })?;

    connection
        .execute_batch(SCHEMA_SQL)
        .map_err(StoreError::Query)?;

    Ok(connection)
}

fn portable_dir() -> Result<PathBuf, StoreError> {
    let exe_path = std::env::current_exe().map_err(StoreError::ExeLocation)?;
    let exe_directory = exe_path.parent().ok_or(StoreError::NoExeDirectory)?;

    Ok(exe_directory.to_path_buf())
}

#[cfg(target_os = "windows")]
fn default_dir() -> Result<PathBuf, StoreError> {
    let app_data = std::env::var_os("APPDATA").ok_or(StoreError::NoDefaultDirectory)?;
    Ok(PathBuf::from(app_data).join("Butter TTS"))
}

#[cfg(target_os = "macos")]
fn default_dir() -> Result<PathBuf, StoreError> {
    let home = std::env::var_os("HOME").ok_or(StoreError::NoDefaultDirectory)?;
    Ok(PathBuf::from(home).join("Library/Application Support/Butter TTS"))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn default_dir() -> Result<PathBuf, StoreError> {
    let home = std::env::var_os("HOME").ok_or(StoreError::NoDefaultDirectory)?;
    Ok(PathBuf::from(home).join(".local/share/Butter TTS"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db(test_name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("butter-tts-store-{test_name}.db"));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn opening_a_fresh_path_creates_the_schema() {
        let path = temp_db("fresh-schema");

        let connection = open_at(&path).expect("should open and create the schema");

        let table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('settings', 'transcripts')",
                [],
                |row| row.get(0),
            )
            .expect("should be able to query sqlite_master");

        assert_eq!(table_count, 2);
    }

    #[test]
    fn opening_the_same_path_twice_does_not_fail() {
        let path = temp_db("reopen");

        open_at(&path).expect("first open should succeed");
        open_at(&path).expect("second open should also succeed");
    }

    #[test]
    fn the_default_directory_resolves_to_somewhere_writable_on_this_os() {
        let directory = default_dir().expect("should resolve on this OS");
        std::fs::create_dir_all(&directory).expect("should be creatable");

        let probe = directory.join("butter-tts-default-dir-test-probe");
        std::fs::write(&probe, b"probe").expect("the directory should be writable");
        std::fs::remove_file(&probe).expect("cleanup should succeed");
    }
}
