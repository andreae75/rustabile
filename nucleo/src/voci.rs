//! Le voci della playlist: file, cartelle, playlist e radio, sempre come URI.
//!
//! Le playlist possono stare anche in rete: chi chiama passa la funzione che
//! legge il contenuto di un indirizzo (GIO sotto GTK, HTTP di Windows sotto WinUI).

use std::path::{Component, Path, PathBuf};

use url::Url;

/// Estensioni dei file che consideriamo musica quando esploriamo una cartella.
const ESTENSIONI_AUDIO: &[&str] = &["mp3", "flac", "ogg", "opus", "m4a", "aac", "wav", "wma"];
/// Estensioni delle playlist che sappiamo leggere.
const ESTENSIONI_PLAYLIST: &[&str] = &["m3u", "m3u8", "pls"];

/// Legge il contenuto di un indirizzo (file o rete); `None` se non ci riesce.
pub type Leggi<'a> = &'a dyn Fn(&str) -> Option<Vec<u8>>;

/// Una voce della playlist: un file locale o uno stream radio, sempre come URI.
#[derive(Clone, Debug)]
pub struct Voce {
    pub uri: String,
    pub nome: String,
    /// La playlist locale (.m3u/.pls) da cui arriva, se c'è: cestinando il brano
    /// ne togliamo anche la riga, se il file della playlist è scrivibile.
    pub origine: Option<PathBuf>,
}

/// Il percorso sul disco di un URI `file://`; `None` per gli indirizzi di rete.
pub fn percorso(uri: &str) -> Option<PathBuf> {
    Url::parse(uri)
        .ok()
        .filter(|u| u.scheme() == "file")
        .and_then(|u| u.to_file_path().ok())
}

/// L'URI `file://` di un percorso (reso assoluto e senza "." o "..").
pub fn uri_da_percorso(percorso: &Path) -> String {
    let assoluto = std::path::absolute(percorso).unwrap_or_else(|_| percorso.to_path_buf());
    let mut pulito = PathBuf::new();
    for parte in assoluto.components() {
        match parte {
            Component::CurDir => {}
            Component::ParentDir => {
                pulito.pop();
            }
            altro => pulito.push(altro),
        }
    }
    Url::from_file_path(&pulito)
        .map(String::from)
        .unwrap_or_else(|_| pulito.to_string_lossy().into_owned())
}

/// Un testo scritto o trascinato: un indirizzo ("http://…") oppure un percorso.
pub fn uri_da_testo(testo: &str) -> String {
    if testo.contains("://") {
        testo.to_string()
    } else {
        uri_da_percorso(Path::new(testo))
    }
}

/// Testo trascinato (per esempio un link da un browser): una voce per riga.
pub fn uri_da_righe(testo: &str) -> Vec<String> {
    testo
        .lines()
        .map(str::trim)
        .filter(|r| !r.is_empty() && !r.starts_with('#'))
        .map(uri_da_testo)
        .collect()
}

pub fn estensione(nome: &str) -> String {
    Path::new(nome.split(['?', '#']).next().unwrap_or(nome))
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Nome leggibile di una voce: per i file il nome senza estensione, per gli stream l'indirizzo.
/// Esercizio: leggere i tag dei file (crate `lofty`).
fn nome_da_uri(uri: &str) -> String {
    match percorso(uri) {
        Some(p) => p
            .file_stem()
            .map(|s| s.to_string_lossy().replace('_', " "))
            .unwrap_or_default(),
        None => uri.to_string(),
    }
}

/// Una voce scritta in una playlist ("album/01.mp3", "/musica/x.flac", "http://…")
/// diventa un URI: i percorsi relativi partono dalla playlist `base`.
fn risolvi_voce(base: &str, voce: &str) -> String {
    if voce.contains("://") {
        return voce.to_string();
    }
    if Path::new(voce).is_absolute() {
        return uri_da_percorso(Path::new(voce));
    }
    let relativa = voce.replace('\\', "/");
    match percorso(base) {
        Some(p) => match p.parent() {
            Some(cartella) => uri_da_percorso(&cartella.join(relativa)),
            None => uri_da_percorso(Path::new(&relativa)),
        },
        None => Url::parse(base)
            .and_then(|b| b.join(&relativa))
            .map(String::from)
            .unwrap_or(relativa),
    }
}

/// Legge una playlist (locale o online) e restituisce le sue voci.
fn leggi_playlist(uri: &str, leggi: Leggi) -> Vec<Voce> {
    let Some(byte) = leggi(uri) else {
        eprintln!("Impossibile leggere la playlist {uri}");
        return Vec::new();
    };
    let testo = String::from_utf8_lossy(&byte);
    // I file salvati da Windows possono iniziare con il "BOM": non fa parte del testo.
    let testo = testo.trim_start_matches('\u{feff}');
    let origine = percorso(uri);
    let voce = |uri: String, nome: String| Voce {
        uri,
        nome,
        origine: origine.clone(),
    };

    let mut voci = Vec::new();
    if estensione(uri) == "pls" {
        // Formato PLS: File1=..., Title1=...
        let mut titoli = std::collections::HashMap::new();
        let mut indirizzi = Vec::new();
        for riga in testo.lines() {
            if let Some((chiave, valore)) = riga.trim().split_once('=') {
                let chiave = chiave.to_lowercase();
                if let Some(n) = chiave.strip_prefix("file") {
                    indirizzi.push((n.to_string(), valore.trim().to_string()));
                } else if let Some(n) = chiave.strip_prefix("title") {
                    titoli.insert(n.to_string(), valore.trim().to_string());
                }
            }
        }
        for (n, indirizzo) in indirizzi {
            let u = risolvi_voce(uri, &indirizzo);
            let nome = titoli.remove(&n).unwrap_or_else(|| nome_da_uri(&u));
            voci.push(voce(u, nome));
        }
    } else {
        // Formato M3U: una voce per riga; "#EXTINF:durata,Titolo" dà il nome alla riga dopo.
        let mut prossimo_nome: Option<String> = None;
        for riga in testo.lines().map(str::trim) {
            if let Some(info) = riga.strip_prefix("#EXTINF:") {
                prossimo_nome = info.split_once(',').map(|(_, t)| t.trim().to_string());
            } else if !riga.is_empty() && !riga.starts_with('#') {
                let u = risolvi_voce(uri, riga);
                let nome = prossimo_nome.take().unwrap_or_else(|| nome_da_uri(&u));
                voci.push(voce(u, nome));
            }
        }
    }
    voci
}

/// Toglie da una playlist locale tutte le voci che puntano a `uri`.
/// Tocca il file solo se lo possiamo scrivere; altrimenti lo lascia com'è.
/// Restituisce `true` se il file è stato riscritto.
pub fn togli_da_playlist(playlist: &Path, uri: &str) -> bool {
    // Si può scrivere? Aprirlo in aggiunta non cambia nulla, ma fallisce se non abbiamo il permesso.
    if std::fs::OpenOptions::new()
        .append(true)
        .open(playlist)
        .is_err()
    {
        return false;
    }
    let Ok(testo) = std::fs::read_to_string(playlist) else {
        return false;
    };
    let base = uri_da_percorso(playlist);
    let punta_al_brano =
        |voce: &str| risolvi_voce(&base, voce.trim().trim_start_matches('\u{feff}')) == uri;
    let a_capo = if testo.contains("\r\n") { "\r\n" } else { "\n" };
    let righe: Vec<&str> = testo.lines().collect();

    let nuove: Vec<String> = if estensione(&playlist.to_string_lossy()) == "pls" {
        // PLS: togliamo FileN/TitleN/LengthN delle voci da eliminare e rinumeriamo il resto.
        let numero = |riga: &str, prefisso: &str| -> Option<String> {
            let (chiave, _) = riga.trim().split_once('=')?;
            let chiave = chiave.to_lowercase();
            let n = chiave.strip_prefix(prefisso)?;
            (!n.is_empty() && n.chars().all(|c| c.is_ascii_digit())).then(|| n.to_string())
        };
        let via: std::collections::HashSet<String> = righe
            .iter()
            .filter_map(|r| {
                let n = numero(r, "file")?;
                punta_al_brano(r.split_once('=')?.1).then_some(n)
            })
            .collect();
        if via.is_empty() {
            return false;
        }
        let mut restanti: Vec<String> = righe.iter().filter_map(|r| numero(r, "file")).collect();
        restanti.retain(|n| !via.contains(n));
        let nuovo_numero = |n: &str| restanti.iter().position(|x| x == n).map(|i| i + 1);
        righe
            .iter()
            .filter_map(|r| {
                for prefisso in ["file", "title", "length"] {
                    if let Some(n) = numero(r, prefisso) {
                        let nuovo = nuovo_numero(&n)?;
                        let (chiave, valore) = r.trim().split_once('=')?;
                        let nome = &chiave[..prefisso.len()];
                        return Some(format!("{nome}{nuovo}={valore}"));
                    }
                }
                if r.trim().to_lowercase().starts_with("numberofentries=") {
                    return Some(format!("NumberOfEntries={}", restanti.len()));
                }
                Some(r.to_string())
            })
            .collect()
    } else {
        // M3U: togliamo la riga della voce e la sua "#EXTINF" subito prima.
        let mut via = vec![false; righe.len()];
        let mut ultima_extinf: Option<usize> = None;
        for (i, riga) in righe.iter().enumerate() {
            let r = riga.trim();
            if r.starts_with("#EXTINF:") {
                ultima_extinf = Some(i);
            } else if !r.is_empty() && !r.starts_with('#') {
                if punta_al_brano(r) {
                    via[i] = true;
                    if let Some(e) = ultima_extinf {
                        via[e] = true;
                    }
                }
                ultima_extinf = None;
            }
        }
        if !via.contains(&true) {
            return false;
        }
        righe
            .iter()
            .zip(via)
            .filter(|(_, togli)| !togli)
            .map(|(r, _)| r.to_string())
            .collect()
    };

    let mut risultato = nuove.join(a_capo);
    if testo.ends_with('\n') {
        risultato.push_str(a_capo);
    }
    match std::fs::write(playlist, risultato) {
        Ok(()) => true,
        Err(e) => {
            eprintln!(
                "Impossibile aggiornare la playlist {}: {e}",
                playlist.display()
            );
            false
        }
    }
}

/// Trasforma un file, una cartella o una playlist in voci da suonare.
/// Le cartelle vengono esplorate (sottocartelle comprese) in ordine alfabetico.
pub fn raccogli(uri: &str, leggi: Leggi, risultato: &mut Vec<Voce>) {
    let est = estensione(uri);
    if ESTENSIONI_PLAYLIST.contains(&est.as_str()) {
        risultato.extend(leggi_playlist(uri, leggi));
        return;
    }
    match percorso(uri) {
        Some(cartella) if cartella.is_dir() => {
            let Ok(voci) = std::fs::read_dir(&cartella) else {
                return;
            };
            let mut figli: Vec<_> = voci.filter_map(|v| v.ok().map(|v| v.path())).collect();
            figli.sort();
            for figlio in figli {
                raccogli(&uri_da_percorso(&figlio), leggi, risultato);
            }
        }
        Some(_) if !ESTENSIONI_AUDIO.contains(&est.as_str()) => {}
        // File audio locale, oppure indirizzo di uno stream.
        _ => risultato.push(Voce {
            uri: uri.to_string(),
            nome: nome_da_uri(uri),
            origine: None,
        }),
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn dal_disco(uri: &str) -> Option<Vec<u8>> {
        std::fs::read(percorso(uri)?).ok()
    }

    fn cartella(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rustabile-test-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("album")).unwrap();
        for f in [
            "album/02 - Due.flac",
            "album/01 - Uno.mp3",
            "album/note.txt",
        ] {
            std::fs::write(d.join(f), b"").unwrap();
        }
        d
    }

    fn voci_di(p: &Path) -> Vec<Voce> {
        let mut voci = Vec::new();
        raccogli(&uri_da_percorso(p), &dal_disco, &mut voci);
        voci
    }

    #[test]
    fn cartella_in_ordine_senza_file_non_audio() {
        let d = cartella("cartella");
        let voci = voci_di(&d.join("album"));
        let nomi: Vec<_> = voci.iter().map(|v| v.nome.as_str()).collect();
        assert_eq!(nomi, ["01 - Uno", "02 - Due"]);
    }

    #[test]
    fn m3u_con_titoli_percorsi_relativi_e_radio() {
        let d = cartella("m3u");
        std::fs::write(
            d.join("lista.m3u"),
            "\u{feff}#EXTM3U\n#EXTINF:123,Artista - Primo\nalbum/01 - Uno.mp3\n\nalbum\\02 - Due.flac\n#EXTINF:-1,Radio di prova\nhttp://radio.example/stream\n",
        )
        .unwrap();
        let voci = voci_di(&d.join("lista.m3u"));
        assert_eq!(voci.len(), 3);
        assert_eq!(voci[0].nome, "Artista - Primo");
        assert!(voci[0].uri.ends_with("album/01%20-%20Uno.mp3"));
        assert_eq!(voci[1].nome, "02 - Due");
        assert!(percorso(&voci[1].uri).unwrap().is_file());
        assert_eq!(voci[2].nome, "Radio di prova");
        assert_eq!(voci[2].uri, "http://radio.example/stream");
    }

    #[test]
    fn pls_con_titoli() {
        let d = cartella("pls");
        std::fs::write(
            d.join("radio.pls"),
            "[playlist]\nFile1=http://radio.example/a\nTitle1=Radio A\nFile2=http://radio.example/b\nNumberOfEntries=2\n",
        )
        .unwrap();
        let voci = voci_di(&d.join("radio.pls"));
        assert_eq!(voci.len(), 2);
        assert_eq!(voci[0].nome, "Radio A");
        assert_eq!(voci[1].uri, "http://radio.example/b");
    }

    #[test]
    fn playlist_in_rete_con_voci_relative() {
        let rete = |_: &str| Some(b"brani/a.mp3\n".to_vec());
        let mut voci = Vec::new();
        raccogli("http://radio.example/x/lista.m3u", &rete, &mut voci);
        assert_eq!(voci[0].uri, "http://radio.example/x/brani/a.mp3");
    }

    fn uri(d: &Path, nome: &str) -> String {
        uri_da_percorso(&d.join(nome))
    }

    #[test]
    fn m3u_toglie_voce_e_extinf() {
        let d = cartella("togli-m3u");
        let lista = d.join("lista.m3u");
        std::fs::write(
            &lista,
            "#EXTM3U\n#EXTINF:100,Uno\n01 Uno.mp3\n#EXTINF:200,Due\n02 Due.flac\nhttp://radio.example/x\n",
        )
        .unwrap();
        assert!(togli_da_playlist(&lista, &uri(&d, "02 Due.flac")));
        assert_eq!(
            std::fs::read_to_string(&lista).unwrap(),
            "#EXTM3U\n#EXTINF:100,Uno\n01 Uno.mp3\nhttp://radio.example/x\n"
        );
        // Una voce che non c'è: il file non cambia.
        assert!(!togli_da_playlist(&lista, &uri(&d, "99 Nessuno.mp3")));
    }

    #[test]
    fn pls_toglie_e_rinumera() {
        let d = cartella("togli-pls");
        let lista = d.join("lista.pls");
        std::fs::write(
            &lista,
            "[playlist]\r\nFile1=a.mp3\r\nTitle1=A\r\nFile2=b.mp3\r\nTitle2=B\r\nLength2=10\r\nFile3=c.mp3\r\nTitle3=C\r\nNumberOfEntries=3\r\nVersion=2\r\n",
        )
        .unwrap();
        assert!(togli_da_playlist(&lista, &uri(&d, "b.mp3")));
        assert_eq!(
            std::fs::read_to_string(&lista).unwrap(),
            "[playlist]\r\nFile1=a.mp3\r\nTitle1=A\r\nFile2=c.mp3\r\nTitle2=C\r\nNumberOfEntries=2\r\nVersion=2\r\n"
        );
    }

    #[test]
    fn sola_lettura_resta_com_e() {
        let d = cartella("solalettura");
        let lista = d.join("lista.m3u");
        // Può restare, in sola lettura, da un giro precedente.
        std::fs::write(&lista, "01 Uno.mp3\n").unwrap();
        let mut permessi = std::fs::metadata(&lista).unwrap().permissions();
        permessi.set_readonly(true);
        std::fs::set_permissions(&lista, permessi).unwrap();
        assert!(!togli_da_playlist(&lista, &uri(&d, "01 Uno.mp3")));
        assert_eq!(std::fs::read_to_string(&lista).unwrap(), "01 Uno.mp3\n");
    }
}
