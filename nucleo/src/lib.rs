//! La parte di Rustabile che non dipende dalla finestra né dal motore audio:
//! playlist, coda, preferenze, lingue e descrizione della qualità del brano.
//! La usano la finestra GTK4 (con GStreamer) e quella WinUI 3 (con l'audio di Windows).

pub mod coda;
pub mod impostazioni;
pub mod lingua;
pub mod qualita;
pub mod voci;

pub use coda::Coda;
pub use impostazioni::Impostazioni;
pub use lingua::{Testo, t};
pub use qualita::{Qualita, mm_ss};
pub use voci::Voce;
