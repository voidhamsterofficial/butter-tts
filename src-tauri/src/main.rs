// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    // Nothing has a window yet if startup fails, so stderr is the only place left to
    // say why. Returning the error would print it via Debug, which is not for reading.
    if let Err(error) = butter_tts_lib::run() {
        eprintln!("Butter TTS could not start: {error}");
        std::process::exit(1);
    }
}
