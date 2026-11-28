//! Le preferenze, salvate in `impostazioni.ini` nella cartella di configurazione:
//! `~/.config/rustabile/` su Linux, `%APPDATA%\rustabile\` su Windows.
//! Il formato è quello del `KeyFile` di GLib, con un solo gruppo.

use std::path::PathBuf;

const GRUPPO: &str = "[Rustabile]";

pub struct Impostazioni {
    /// Trascinando file nella finestra: suonano subito (vero) o vanno in coda (falso).
    pub trascina_suona: bool,
    /// Il pannello playlist era aperto all'ultima chiusura.
    pub mostra_playlist: bool,
    /// Lingua dell'interfaccia: "auto" (quella del desktop) o "it", "en", "de", "es", "fr".
    pub lingua: String,
    pub volume: f64,
}

impl Default for Impostazioni {
    fn default() -> Self {
        Impostazioni {
            trascina_suona: true,
            mostra_playlist: false,
            lingua: "auto".into(),
            volume: 0.8,
        }
    }
}

impl Impostazioni {
    fn percorso() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_default()
            .join("rustabile")
            .join("impostazioni.ini")
    }

    pub fn carica() -> Self {
        Self::da_testo(&std::fs::read_to_string(Self::percorso()).unwrap_or_default())
    }

    pub fn salva(&self) {
        let percorso = Self::percorso();
        if let Some(cartella) = percorso.parent() {
            let _ = std::fs::create_dir_all(cartella);
        }
        if let Err(e) = std::fs::write(&percorso, self.testo()) {
            eprintln!("Impossibile salvare le impostazioni: {e}");
        }
    }

    fn da_testo(testo: &str) -> Self {
        let mut imp = Self::default();
        let mut dentro = false;
        for riga in testo.lines().map(str::trim) {
            if riga.starts_with('[') {
                dentro = riga == GRUPPO;
                continue;
            }
            let Some((chiave, valore)) = riga.split_once('=').filter(|_| dentro) else {
                continue;
            };
            let valore = valore.trim();
            match chiave.trim() {
                "trascina_suona" => imp.trascina_suona = valore.parse().unwrap_or(true),
                "mostra_playlist" => imp.mostra_playlist = valore.parse().unwrap_or(false),
                "lingua" => imp.lingua = valore.to_string(),
                "volume" => imp.volume = valore.parse().unwrap_or(0.8),
                _ => {}
            }
        }
        imp
    }

    fn testo(&self) -> String {
        format!(
            "{GRUPPO}\ntrascina_suona={}\nmostra_playlist={}\nlingua={}\nvolume={}\n",
            self.trascina_suona, self.mostra_playlist, self.lingua, self.volume
        )
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn legge_il_file_scritto_da_glib() {
        let imp = Impostazioni::da_testo(
            "[Rustabile]\ntrascina_suona=false\nmostra_playlist=true\nlingua=de\nvolume=0.35000000000000003\n",
        );
        assert!(!imp.trascina_suona && imp.mostra_playlist);
        assert_eq!(imp.lingua, "de");
        assert!((imp.volume - 0.35).abs() < 1e-9);
        let riletto = Impostazioni::da_testo(&imp.testo());
        assert_eq!(riletto.testo(), imp.testo());
    }
}
