//! Traduzioni dell'interfaccia: italiano, inglese, tedesco, spagnolo, francese.
//!
//! Per un programma piccolo basta una tabella nel codice: niente file .po da
//! installare. La lingua si sceglie da quella del desktop (la finestra la passa
//! a `inizializza`), a meno che nelle Preferenze se ne sia scelta una;
//! se non è una delle cinque si usa l'inglese.
//! Per provarne un'altra: `RUSTABILE_LINGUA=de rustabile`.

use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Lingua {
    It,
    En,
    De,
    Es,
    Fr,
}

impl Lingua {
    fn da_codice(codice: &str) -> Option<Self> {
        match codice.get(..2)?.to_ascii_lowercase().as_str() {
            "it" => Some(Lingua::It),
            "en" => Some(Lingua::En),
            "de" => Some(Lingua::De),
            "es" => Some(Lingua::Es),
            "fr" => Some(Lingua::Fr),
            _ => None,
        }
    }
}

static LINGUA: OnceLock<Lingua> = OnceLock::new();

/// Sceglie la lingua, una volta sola, all'avvio: la variabile `RUSTABILE_LINGUA`,
/// poi la scelta nelle Preferenze (`scelta`, se non è "auto"), poi quelle del sistema.
pub fn inizializza<S: AsRef<str>>(scelta: &str, di_sistema: &[S]) {
    LINGUA.get_or_init(|| {
        std::env::var("RUSTABILE_LINGUA")
            .ok()
            .and_then(|v| Lingua::da_codice(&v))
            .or_else(|| Lingua::da_codice(scelta))
            .or_else(|| {
                di_sistema
                    .iter()
                    .find_map(|nome| Lingua::da_codice(nome.as_ref()))
            })
            .unwrap_or(Lingua::En)
    });
}

pub fn lingua() -> Lingua {
    LINGUA.get().copied().unwrap_or(Lingua::En)
}

/// Ogni testo dell'interfaccia ha una voce qui.
#[derive(Clone, Copy)]
pub enum Testo {
    TrascinaQui,
    IndirizzoRadio,
    Ascolta,
    AggiungiRadio,
    Svuota,
    PlaylistVuota,
    MostraPlaylist,
    Indietro,
    PlayPausa,
    Stop,
    Avanti,
    Volume,
    Menu,
    Preferenze,
    Informazioni,
    ApriCartella,
    SalvaPlaylist,
    PlaylistSalvata,
    ErroreSalvataggio,
    TastoSpazio,
    Cestina,
    SpostatoNelCestino,
    ErroreCestino,
    TrascinaSuona,
    TrascinaSuonaSpiegazione,
    Descrizione,
    Mono,
    Stereo,
    TitoloLingua,
    LinguaAutomatica,
    LinguaSpiegazione,
    DescrizioneWindows,
    Chiudi,
    Caricamento,
}

pub fn t(testo: Testo) -> &'static str {
    use Lingua::*;
    use Testo::*;
    let l = lingua();
    match testo {
        TrascinaQui => match l {
            It => "Trascina qui musica, cartelle o playlist",
            En => "Drop music, folders or playlists here",
            De => "Musik, Ordner oder Wiedergabelisten hierher ziehen",
            Es => "Arrastra aquí música, carpetas o listas",
            Fr => "Déposez ici musique, dossiers ou listes de lecture",
        },
        IndirizzoRadio => match l {
            It => "https://… indirizzo della radio",
            En => "https://… radio stream address",
            De => "https://… Adresse des Radiosenders",
            Es => "https://… dirección de la radio",
            Fr => "https://… adresse de la radio",
        },
        Ascolta => match l {
            It => "Ascolta",
            En => "Listen",
            De => "Anhören",
            Es => "Escuchar",
            Fr => "Écouter",
        },
        AggiungiRadio => match l {
            It => "Aggiungi una radio",
            En => "Add a radio station",
            De => "Radiosender hinzufügen",
            Es => "Añadir una radio",
            Fr => "Ajouter une radio",
        },
        Svuota => match l {
            It => "Svuota",
            En => "Clear",
            De => "Leeren",
            Es => "Vaciar",
            Fr => "Vider",
        },
        PlaylistVuota => match l {
            It => "Playlist vuota",
            En => "Empty playlist",
            De => "Wiedergabeliste leer",
            Es => "Lista vacía",
            Fr => "Liste vide",
        },
        MostraPlaylist => match l {
            It => "Mostra la playlist",
            En => "Show the playlist",
            De => "Wiedergabeliste anzeigen",
            Es => "Mostrar la lista",
            Fr => "Afficher la liste de lecture",
        },
        Indietro => match l {
            It => "Indietro",
            En => "Previous",
            De => "Zurück",
            Es => "Anterior",
            Fr => "Précédent",
        },
        PlayPausa => match l {
            It => "Play / Pausa",
            En => "Play / Pause",
            De => "Wiedergabe / Pause",
            Es => "Reproducir / Pausa",
            Fr => "Lecture / Pause",
        },
        Stop => match l {
            It => "Stop",
            En => "Stop",
            De => "Stopp",
            Es => "Detener",
            Fr => "Arrêt",
        },
        Avanti => match l {
            It => "Avanti",
            En => "Next",
            De => "Weiter",
            Es => "Siguiente",
            Fr => "Suivant",
        },
        Volume => match l {
            It => "Volume",
            En => "Volume",
            De => "Lautstärke",
            Es => "Volumen",
            Fr => "Volume",
        },
        Menu => match l {
            It => "Menu",
            En => "Menu",
            De => "Menü",
            Es => "Menú",
            Fr => "Menu",
        },
        Preferenze => match l {
            It => "Preferenze",
            En => "Preferences",
            De => "Einstellungen",
            Es => "Preferencias",
            Fr => "Préférences",
        },
        Cestina => match l {
            It => "Sposta il brano nel Cestino",
            En => "Move track to Trash",
            De => "Titel in den Papierkorb verschieben",
            Es => "Mover la canción a la papelera",
            Fr => "Mettre le morceau à la corbeille",
        },
        SpostatoNelCestino => match l {
            It => "Spostato nel Cestino:",
            En => "Moved to Trash:",
            De => "In den Papierkorb verschoben:",
            Es => "Movido a la papelera:",
            Fr => "Mis à la corbeille :",
        },
        ErroreCestino => match l {
            It => "Impossibile spostare nel Cestino:",
            En => "Could not move to Trash:",
            De => "Verschieben in den Papierkorb nicht möglich:",
            Es => "No se pudo mover a la papelera:",
            Fr => "Impossible de mettre à la corbeille :",
        },
        TastoSpazio => match l {
            It => "Spazio",
            En => "Space",
            De => "Leertaste",
            Es => "Espacio",
            Fr => "Espace",
        },
        ApriCartella => match l {
            It => "Apri cartella…",
            En => "Open Folder…",
            De => "Ordner öffnen…",
            Es => "Abrir carpeta…",
            Fr => "Ouvrir un dossier…",
        },
        SalvaPlaylist => match l {
            It => "Salva playlist…",
            En => "Save Playlist…",
            De => "Wiedergabeliste speichern…",
            Es => "Guardar lista…",
            Fr => "Enregistrer la liste…",
        },
        PlaylistSalvata => match l {
            It => "Playlist salvata:",
            En => "Playlist saved:",
            De => "Wiedergabeliste gespeichert:",
            Es => "Lista guardada:",
            Fr => "Liste enregistrée :",
        },
        ErroreSalvataggio => match l {
            It => "Impossibile salvare:",
            En => "Could not save:",
            De => "Speichern nicht möglich:",
            Es => "No se pudo guardar:",
            Fr => "Impossible d'enregistrer :",
        },
        Informazioni => match l {
            It => "Informazioni su Rustabile",
            En => "About Rustabile",
            De => "Über Rustabile",
            Es => "Acerca de Rustabile",
            Fr => "À propos de Rustabile",
        },
        TrascinaSuona => match l {
            It => "Trascinando, suona subito",
            En => "Play dropped files right away",
            De => "Abgelegte Dateien sofort abspielen",
            Es => "Reproducir al arrastrar",
            Fr => "Lire aussitôt les fichiers déposés",
        },
        TrascinaSuonaSpiegazione => match l {
            It => {
                "Se è spento, i file trascinati vanno in fondo alla playlist senza interrompere il brano in corso."
            }
            En => {
                "When off, dropped files are added to the end of the playlist without interrupting the current track."
            }
            De => {
                "Wenn aus, werden abgelegte Dateien ans Ende der Wiedergabeliste gestellt, ohne den aktuellen Titel zu unterbrechen."
            }
            Es => {
                "Si está desactivado, los archivos arrastrados se añaden al final de la lista sin interrumpir la canción actual."
            }
            Fr => {
                "Si désactivé, les fichiers déposés sont ajoutés à la fin de la liste sans interrompre le morceau en cours."
            }
        },
        Descrizione => match l {
            It => {
                "Un mini lettore musicale per GNOME, semplice come i lettori di una volta.\nScritto in Rust con GTK4 e GStreamer."
            }
            En => {
                "A tiny music player for GNOME, as simple as the players of the old days.\nWritten in Rust with GTK4 and GStreamer."
            }
            De => {
                "Ein kleiner Musikspieler für GNOME, so einfach wie die Player von früher.\nGeschrieben in Rust mit GTK4 und GStreamer."
            }
            Es => {
                "Un pequeño reproductor de música para GNOME, sencillo como los de antes.\nEscrito en Rust con GTK4 y GStreamer."
            }
            Fr => {
                "Un petit lecteur de musique pour GNOME, simple comme les lecteurs d'antan.\nÉcrit en Rust avec GTK4 et GStreamer."
            }
        },
        DescrizioneWindows => match l {
            It => {
                "Un mini lettore musicale, semplice come i lettori di una volta.\nScritto in Rust con WinUI 3 e l'audio di Windows."
            }
            En => {
                "A tiny music player, as simple as the players of the old days.\nWritten in Rust with WinUI 3 and Windows audio."
            }
            De => {
                "Ein kleiner Musikspieler, so einfach wie die Player von früher.\nGeschrieben in Rust mit WinUI 3 und dem Windows-Audio."
            }
            Es => {
                "Un pequeño reproductor de música, sencillo como los de antes.\nEscrito en Rust con WinUI 3 y el audio de Windows."
            }
            Fr => {
                "Un petit lecteur de musique, simple comme les lecteurs d'antan.\nÉcrit en Rust avec WinUI 3 et l'audio de Windows."
            }
        },
        Chiudi => match l {
            It => "Chiudi",
            En => "Close",
            De => "Schließen",
            Es => "Cerrar",
            Fr => "Fermer",
        },
        Caricamento => match l {
            It => "Caricamento…",
            En => "Loading…",
            De => "Wird geladen…",
            Es => "Cargando…",
            Fr => "Chargement…",
        },
        TitoloLingua => match l {
            It => "Lingua",
            En => "Language",
            De => "Sprache",
            Es => "Idioma",
            Fr => "Langue",
        },
        LinguaAutomatica => match l {
            It => "Automatica",
            En => "Automatic",
            De => "Automatisch",
            Es => "Automático",
            Fr => "Automatique",
        },
        LinguaSpiegazione => match l {
            It => "Vale dalla prossima apertura.",
            En => "Takes effect the next time Rustabile starts.",
            De => "Wirksam ab dem nächsten Start.",
            Es => "Se aplica la próxima vez que abras Rustabile.",
            Fr => "Prend effet au prochain lancement.",
        },
        Mono => "mono",
        Stereo => match l {
            Es => "estéreo",
            Fr => "stéréo",
            _ => "stereo",
        },
    }
}

/// "12 brani", "12 tracks", "12 Titel"…
pub fn brani(n: usize) -> String {
    use Lingua::*;
    let l = lingua();
    match (l, n) {
        (_, 0) => t(Testo::PlaylistVuota).to_string(),
        (It, 1) => "1 brano".into(),
        (It, n) => format!("{n} brani"),
        (En, 1) => "1 track".into(),
        (En, n) => format!("{n} tracks"),
        (De, n) => format!("{n} Titel"),
        (Es, 1) => "1 canción".into(),
        (Es, n) => format!("{n} canciones"),
        (Fr, 1) => "1 morceau".into(),
        (Fr, n) => format!("{n} morceaux"),
    }
}

/// "6 canali", "6 channels"…
pub fn canali_n(n: i32) -> String {
    match lingua() {
        Lingua::It => format!("{n} canali"),
        Lingua::En => format!("{n} channels"),
        Lingua::De => format!("{n} Kanäle"),
        Lingua::Es => format!("{n} canales"),
        Lingua::Fr => format!("{n} canaux"),
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn codici_di_lingua() {
        assert_eq!(Lingua::da_codice("it_IT.UTF-8"), Some(Lingua::It));
        assert_eq!(Lingua::da_codice("de"), Some(Lingua::De));
        assert_eq!(Lingua::da_codice("fr_CA"), Some(Lingua::Fr));
        assert_eq!(Lingua::da_codice("pt_BR"), None);
        assert_eq!(Lingua::da_codice("C"), None);
    }
}
