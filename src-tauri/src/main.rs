// Ukrywa okno konsoli w wersji release na Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    open_music_tagger_lib::run()
}
