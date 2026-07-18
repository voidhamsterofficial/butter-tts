//! Everything the app keeps: one SQLite database holding both the settings and the
//! history, encrypted where it matters. See [`crypto`] for the encryption and
//! [`settings`] / [`transcripts`] for what is actually stored.
//!
//! # Where the database lives
//!
//! The app asks the user to pick a spot the first time it runs (see [`needs_setup`] /
//! [`set_up`]):
//!
//! - **Portable**: next to the running executable. Copy that folder to a USB stick and
//!   it works the same on the next machine, leaving nothing behind on this one. This is
//!   the only sane choice for the portable Windows build; on an installed app it works
//!   too, but the folder disappears when the app is reinstalled or updated.
//! - **Default**: the OS's own per-user application data folder, which survives
//!   reinstalls and updates.
//!
//! Later, from the settings page, the user can move the database into the default folder
//! or into any folder they choose (see [`relocate_to_default`] / [`relocate_to`]).
//!
//! A lookup checks three places, in order: a folder the user chose, recorded in a small
//! pointer file in the default folder (see [`LOCATION_POINTER_FILE_NAME`]); then next to
//! the executable; then the default folder. Whichever actually holds a database wins. The
//! two built-in spots need nothing remembered — they are the obvious places to look — so
//! the pointer file exists only when the database has been moved somewhere else.

pub mod crypto;
pub mod settings;
pub mod transcripts;

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Serialize;

const DATABASE_FILE_NAME: &str = "butter-tts.db";

/// Written in the default folder to record a database that lives somewhere else. Its
/// contents are the absolute path of the directory holding the database; its absence means
/// the database is in one of the two built-in spots. See [`find_existing`].
const LOCATION_POINTER_FILE_NAME: &str = "butter-tts.location";

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

    #[error("could not move the database to {path}: {source}")]
    MoveDatabase {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not remember the database location at {path}: {source}")]
    WritePointer {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("that is not a folder I can move the database into")]
    NotADirectory { path: PathBuf },

    #[error("there is already a database in {path}")]
    TargetExists { path: PathBuf },

    #[error("no database has been set up yet")]
    NotSetUp,
}

/// The two places [`set_up`] will put the database on first run. See the module docs for
/// what each one means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Location {
    Portable,
    Default,
}

/// Where the database sits now, as the settings page thinks of it: either the OS's default
/// folder, or some other folder the user chose. Serialised in lowercase so the frontend
/// reads `"default"` / `"custom"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Placement {
    Default,
    Custom,
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

/// Whether the database is in the default folder or somewhere the user chose, so the
/// settings page can mark the current option. `None` before setup.
pub fn current_location() -> Option<Placement> {
    let path = find_existing()?;
    let directory = path.parent()?;

    if is_default_directory(directory) {
        return Some(Placement::Default);
    }

    Some(Placement::Custom)
}

/// Moves the database back to the OS's default folder and returns its new path.
///
/// The caller must not do this while the bot is running: the database is written to during
/// a session, and moving the file out from under a write would lose it. The settings page
/// only offers this while the bot is asleep.
pub fn relocate_to_default() -> Result<PathBuf, StoreError> {
    let default_directory = default_dir()?;
    move_database_into(&default_directory)
}

/// Moves the database into a folder the user picked and returns its new path. Same
/// bot-must-be-asleep rule as [`relocate_to_default`].
pub fn relocate_to(directory: PathBuf) -> Result<PathBuf, StoreError> {
    if !directory.is_dir() {
        return Err(StoreError::NotADirectory { path: directory });
    }

    move_database_into(&directory)
}

/// The move shared by both relocate paths: shift the file if it is not already there, then
/// record where it landed so the next launch can find it.
fn move_database_into(target_directory: &Path) -> Result<PathBuf, StoreError> {
    let current = find_existing().ok_or(StoreError::NotSetUp)?;
    let target = target_directory.join(DATABASE_FILE_NAME);

    if current != target {
        std::fs::create_dir_all(target_directory).map_err(|source| {
            StoreError::CreateDirectory {
                path: target_directory.to_path_buf(),
                source,
            }
        })?;

        // Never write over a database already sitting there — that would be someone else's
        // data, or an older copy the user forgot about.
        if target.exists() {
            return Err(StoreError::TargetExists {
                path: target_directory.to_path_buf(),
            });
        }

        move_file(&current, &target)?;
    }

    remember_location(target_directory)?;

    Ok(target)
}

/// Records where the database now lives: a pointer file for a chosen folder, or nothing for
/// the default one — the pointer's *absence* is what means "the default place".
fn remember_location(directory: &Path) -> Result<(), StoreError> {
    if is_default_directory(directory) {
        clear_pointer();
        return Ok(());
    }

    write_pointer(directory)
}

/// Moves a file, falling back to copy-then-delete when a plain rename cannot cross
/// filesystems — which is exactly the case that matters here, moving between the system
/// disk and a USB stick the portable copy might live on.
fn move_file(from: &Path, to: &Path) -> Result<(), StoreError> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }

    std::fs::copy(from, to).map_err(|source| StoreError::MoveDatabase {
        path: to.to_path_buf(),
        source,
    })?;

    std::fs::remove_file(from).map_err(|source| StoreError::MoveDatabase {
        path: from.to_path_buf(),
        source,
    })?;

    Ok(())
}

/// Finds the database wherever it ended up, in the order it might be: a folder the user
/// chose (recorded in the pointer file), then next to the app, then the default folder.
/// Whichever actually holds the file wins.
fn find_existing() -> Option<PathBuf> {
    if let Some(directory) = read_pointer() {
        if let Some(found) = database_in(&directory) {
            return Some(found);
        }
    }

    if let Ok(directory) = portable_dir() {
        if let Some(found) = database_in(&directory) {
            return Some(found);
        }
    }

    if let Ok(directory) = default_dir() {
        if let Some(found) = database_in(&directory) {
            return Some(found);
        }
    }

    None
}

/// The database's path inside `directory`, if the file is actually there.
fn database_in(directory: &Path) -> Option<PathBuf> {
    let candidate = directory.join(DATABASE_FILE_NAME);
    candidate.exists().then_some(candidate)
}

/// Whether `directory` is the OS's default folder, comparing through the real filesystem so
/// a trailing slash or a symlink does not read as a different place. Falls back to a plain
/// comparison when a path cannot be resolved (e.g. the default folder does not exist yet).
fn is_default_directory(directory: &Path) -> bool {
    let Ok(default) = default_dir() else {
        return false;
    };

    match (
        std::fs::canonicalize(directory),
        std::fs::canonicalize(&default),
    ) {
        (Ok(resolved), Ok(resolved_default)) => resolved == resolved_default,
        _ => directory == default,
    }
}

/// The pointer file's own path, inside the default folder — the one place always checked,
/// so a record left there is always found.
fn pointer_path() -> Result<PathBuf, StoreError> {
    Ok(default_dir()?.join(LOCATION_POINTER_FILE_NAME))
}

/// The directory the pointer file names, or `None` if there is no usable pointer.
fn read_pointer() -> Option<PathBuf> {
    let contents = std::fs::read_to_string(pointer_path().ok()?).ok()?;
    let directory = contents.trim();

    if directory.is_empty() {
        return None;
    }

    Some(PathBuf::from(directory))
}

/// Records a chosen directory in the pointer file, creating the default folder to hold it
/// if need be.
fn write_pointer(directory: &Path) -> Result<(), StoreError> {
    let pointer = pointer_path()?;

    if let Some(parent) = pointer.parent() {
        std::fs::create_dir_all(parent).map_err(|source| StoreError::CreateDirectory {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    std::fs::write(&pointer, directory.to_string_lossy().as_bytes()).map_err(|source| {
        StoreError::WritePointer {
            path: pointer,
            source,
        }
    })
}

/// Removes the pointer file, so lookups fall back to the default folder. A missing pointer
/// is already the state we want, so that is not an error.
fn clear_pointer() {
    if let Ok(pointer) = pointer_path() {
        let _ = std::fs::remove_file(pointer);
    }
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

    #[test]
    fn moving_a_file_carries_its_contents_and_leaves_nothing_behind() {
        let from = temp_db("move-from");
        let to = temp_db("move-to");
        std::fs::write(&from, b"the database").expect("should write the source");

        move_file(&from, &to).expect("the move should succeed");

        assert!(!from.exists(), "the original should be gone after a move");
        assert_eq!(
            std::fs::read(&to).expect("the destination should exist"),
            b"the database",
            "the contents should survive the move",
        );

        let _ = std::fs::remove_file(&to);
    }

    #[test]
    fn a_directory_holds_the_database_only_when_the_file_is_actually_there() {
        let directory = std::env::temp_dir().join("butter-tts-db-in-test");
        std::fs::create_dir_all(&directory).expect("should make the test directory");
        let _ = std::fs::remove_file(directory.join(DATABASE_FILE_NAME));

        assert!(
            database_in(&directory).is_none(),
            "empty directory holds nothing"
        );

        std::fs::write(directory.join(DATABASE_FILE_NAME), b"db").expect("should write a db");
        assert_eq!(
            database_in(&directory),
            Some(directory.join(DATABASE_FILE_NAME))
        );

        let _ = std::fs::remove_file(directory.join(DATABASE_FILE_NAME));
    }

    #[test]
    fn the_default_folder_is_recognised_and_a_random_folder_is_not() {
        let default = default_dir().expect("should resolve on this OS");
        std::fs::create_dir_all(&default).expect("should be creatable");

        assert!(
            is_default_directory(&default),
            "the default folder should be recognised as itself",
        );
        assert!(
            !is_default_directory(&std::env::temp_dir()),
            "an unrelated folder should not read as the default",
        );
    }
}
