//! End-to-end check of moving the database around, run in its own process so it can point
//! `HOME` at a throwaway folder without disturbing the real application-support folder.
//!
//! Unix only: `default_dir()` is built from `HOME` there, which is the hook this test uses
//! to sandbox itself. On Windows the same path is driven by `APPDATA`.
#![cfg(unix)]

use butter_tts_lib::store::{self, Location, Placement};

#[test]
fn the_database_moves_to_a_chosen_folder_and_back() {
    let sandbox = std::env::temp_dir().join("butter-tts-relocate-it");
    let _ = std::fs::remove_dir_all(&sandbox);
    let home = sandbox.join("home");
    let chosen = sandbox.join("chosen");
    std::fs::create_dir_all(&home).expect("should make the sandbox home");
    std::fs::create_dir_all(&chosen).expect("should make the chosen folder");

    // Everything the store does with the default folder now lands inside the sandbox.
    std::env::set_var("HOME", &home);

    // Starts life in the default folder.
    let default_path = store::set_up(Location::Default).expect("set up should succeed");
    assert!(default_path.exists());
    assert_eq!(store::current_location(), Some(Placement::Default));

    // Moved into the folder the user picked.
    let moved = store::relocate_to(chosen.clone()).expect("relocate should succeed");
    assert_eq!(moved, chosen.join("butter-tts.db"));
    assert!(
        moved.exists(),
        "the database should now be in the chosen folder"
    );
    assert!(
        !default_path.exists(),
        "the database should be gone from the default folder",
    );
    assert_eq!(store::current_location(), Some(Placement::Custom));
    // A fresh launch finds it again through the pointer file.
    assert_eq!(
        store::database_path().expect("should find the moved db"),
        moved
    );

    // And back to the default folder, which clears the pointer.
    let back = store::relocate_to_default().expect("relocate back should succeed");
    assert_eq!(back, default_path);
    assert!(back.exists());
    assert!(
        !moved.exists(),
        "the database should be gone from the chosen folder"
    );
    assert_eq!(store::current_location(), Some(Placement::Default));

    let _ = std::fs::remove_dir_all(&sandbox);
}
