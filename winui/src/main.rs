//! Rustabile per Windows: finestra WinUI 3 (libreria `windows-reactor`) e audio
//! di Windows. Playlist, coda, preferenze e lingue stanno in `nucleo`, comune
//! alla versione GTK4.

// Nella versione finale niente finestra nera del terminale.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod audio;
#[cfg(windows)]
mod finestra;
#[cfg(windows)]
mod icona;
#[cfg(windows)]
mod tasti;
#[cfg(windows)]
mod trascina;
#[cfg(windows)]
mod unica;

#[cfg(windows)]
fn main() {
    finestra::avvia();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("rustabile-winui esiste solo per Windows: qui usa `cargo run -p rustabile`.");
}
