//! Rustabile: un mini lettore musicale per GNOME, in Rust.
//!
//! GTK4 per la finestra, GStreamer (`playbin`) per l'audio.
//! - Trascina file, cartelle o playlist (.m3u, .m3u8, .pls) nella finestra.
//! - Il pulsante "radio" accetta l'indirizzo di uno stream o di una playlist online.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

mod lingua;

use gst::prelude::*;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use lingua::{Testo, t};

const APP_ID: &str = "io.github.andreae75.Rustabile";

/// Estensioni dei file che consideriamo musica quando esploriamo una cartella.
const ESTENSIONI_AUDIO: &[&str] = &["mp3", "flac", "ogg", "opus", "m4a", "aac", "wav", "wma"];
/// Estensioni delle playlist che sappiamo leggere.
const ESTENSIONI_PLAYLIST: &[&str] = &["m3u", "m3u8", "pls"];

/// Una voce della playlist: un file locale o uno stream radio, sempre come URI.
struct Voce {
    uri: String,
    nome: String,
}

/// Tutto lo stato del lettore: il motore audio, la playlist e i widget da aggiornare.
///
/// Sta dentro un `Rc<RefCell<Stato>>` perché lo condividono tutte le callback
/// (pulsanti, timer, messaggi di GStreamer): `Rc` per avere più proprietari,
/// `RefCell` per poterlo modificare anche se la callback ne ha solo un riferimento.
struct Stato {
    playbin: gst::Element,
    playlist: Vec<Voce>,
    corrente: Option<usize>,
    titolo: gtk::Label,
    tempo: gtk::Label,
    barra: gtk::Scale,
    pulsante_play: gtk::Button,
    /// Riga piccola con formato e qualità: "FLAC · 16 bit · 44,1 kHz · stereo · 912 kbps".
    dettagli: gtk::Label,
    qualita: Qualita,
    impostazioni: Impostazioni,
    /// Il pannello playlist: un elenco di testi ("▶ 3. Titolo") mostrato da una ListView.
    elenco: gtk::StringList,
    vista: gtk::ListView,
    segno: Rc<Segno>,
    conteggio: gtk::Label,
    /// Finché esiste, GStreamer ci consegna i suoi messaggi (fine brano, errori, tag).
    _guardia_bus: Option<gst::bus::BusWatchGuard>,
}

/// Quello che sappiamo del brano in corso: arriva a pezzi, dai tag e dal decoder.
#[derive(Default)]
struct Qualita {
    /// Nome del codec dai tag ("Free Lossless Audio Codec (FLAC)").
    codec: Option<String>,
    /// Bitrate in bit/s dai tag (MP3 e radio di solito lo dichiarano).
    bitrate: Option<u32>,
    /// Dimensione del file locale, per stimare il bitrate se i tag non lo dicono.
    dimensione: Option<u64>,
    /// Estensione, come ripiego per il nome del formato.
    estensione: String,
    /// Vero per gli stream (radio): cambia come mostriamo titolo e stazione.
    radio: bool,
    /// Nome della stazione, se la radio lo trasmette (tag "organization", da icy-name).
    stazione: Option<String>,
    /// Brano in onda, se la radio lo trasmette (tag "title", da StreamTitle).
    in_onda: Option<String>,
}

impl Qualita {
    /// "Free Lossless Audio Codec (FLAC)" → "FLAC"; altrimenti il nome così com'è.
    fn formato(&self) -> String {
        if let Some(codec) = &self.codec {
            if let (Some(a), Some(b)) = (codec.rfind('('), codec.rfind(')')) {
                if a < b {
                    return codec[a + 1..b].to_string();
                }
            }
            return codec.clone();
        }
        self.estensione.to_uppercase()
    }

    fn senza_perdita(&self) -> bool {
        let f = self.formato().to_uppercase();
        ["FLAC", "WAV", "PCM", "ALAC", "APE", "WAVPACK"]
            .iter()
            .any(|x| f.contains(x))
    }
}

/// Le preferenze, salvate in `~/.config/rustabile/impostazioni.ini`
/// con il `KeyFile` di GLib (lo stesso formato dei file .desktop).
struct Impostazioni {
    /// Finestra sempre sopra le altre (con Xwayland, vale dalla prossima apertura).
    sempre_sopra: bool,
    /// Trascinando file nella finestra: suonano subito (vero) o vanno in coda (falso).
    trascina_suona: bool,
    /// Il pannello playlist era aperto all'ultima chiusura.
    mostra_playlist: bool,
    /// Lingua dell'interfaccia: "auto" (quella del desktop) o "it", "en", "de", "es", "fr".
    lingua: String,
    volume: f64,
}

impl Impostazioni {
    const GRUPPO: &str = "Rustabile";

    fn percorso() -> std::path::PathBuf {
        glib::user_config_dir()
            .join("rustabile")
            .join("impostazioni.ini")
    }

    fn carica() -> Self {
        let kf = glib::KeyFile::new();
        let _ = kf.load_from_file(Self::percorso(), glib::KeyFileFlags::NONE);
        Impostazioni {
            sempre_sopra: kf.boolean(Self::GRUPPO, "sempre_sopra").unwrap_or(false),
            trascina_suona: kf.boolean(Self::GRUPPO, "trascina_suona").unwrap_or(true),
            mostra_playlist: kf.boolean(Self::GRUPPO, "mostra_playlist").unwrap_or(false),
            lingua: kf
                .string(Self::GRUPPO, "lingua")
                .map(|s| s.to_string())
                .unwrap_or_else(|_| "auto".into()),
            volume: kf.double(Self::GRUPPO, "volume").unwrap_or(0.8),
        }
    }

    fn salva(&self) {
        let kf = glib::KeyFile::new();
        kf.set_boolean(Self::GRUPPO, "sempre_sopra", self.sempre_sopra);
        kf.set_boolean(Self::GRUPPO, "trascina_suona", self.trascina_suona);
        kf.set_boolean(Self::GRUPPO, "mostra_playlist", self.mostra_playlist);
        kf.set_string(Self::GRUPPO, "lingua", &self.lingua);
        kf.set_double(Self::GRUPPO, "volume", self.volume);
        let percorso = Self::percorso();
        if let Some(cartella) = percorso.parent() {
            let _ = std::fs::create_dir_all(cartella);
        }
        if let Err(e) = kf.save_to_file(&percorso) {
            eprintln!("Impossibile salvare le impostazioni: {e}");
        }
    }
}

/// "44100" → "44,1 kHz", "96000" → "96 kHz".
fn khz(hz: i32) -> String {
    let k = hz as f64 / 1000.0;
    if k.fract() == 0.0 {
        format!("{k:.0} kHz")
    } else {
        format!("{k:.1} kHz").replace('.', ",")
    }
}

fn canali(n: i32) -> String {
    match n {
        1 => t(Testo::Mono).into(),
        2 => t(Testo::Stereo).into(),
        n => lingua::canali_n(n),
    }
}

/// Bit per campione dal formato audio decodificato ("S16LE" → 16, "S24_32LE" → 24).
fn bit(formato: &str) -> Option<u32> {
    let cifre: String = formato
        .chars()
        .skip(1)
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if formato.starts_with('S') || formato.starts_with('U') {
        cifre.parse().ok()
    } else {
        None
    }
}

impl Stato {
    /// Fa partire la voce numero `indice` della playlist.
    fn suona(&mut self, indice: usize) {
        let Some(voce) = self.playlist.get(indice) else {
            return;
        };
        let _ = self.playbin.set_state(gst::State::Null);
        self.playbin.set_property("uri", voce.uri.as_str());
        let _ = self.playbin.set_state(gst::State::Playing);

        let file = gio::File::for_uri(&voce.uri);
        self.qualita = Qualita {
            dimensione: file
                .path()
                .and_then(|p| std::fs::metadata(p).ok())
                .map(|m| m.len()),
            estensione: estensione(&voce.uri),
            radio: file.path().is_none(),
            ..Default::default()
        };
        self.dettagli.set_label("");
        // Con una radio non si può andare avanti e indietro: barra grigia da subito.
        self.barra.set_sensitive(!self.qualita.radio);
        self.barra.set_value(0.0);

        let precedente = self.corrente.replace(indice);
        self.titolo
            .set_label(&format!("{}. {}", indice + 1, voce.nome));
        self.pulsante_play
            .set_icon_name("media-playback-pause-symbolic");

        // Nel pannello: togli il segno dal brano di prima, mettilo a quello nuovo.
        self.segno.corrente.set(Some(indice as u32));
        if let Some(p) = precedente {
            self.aggiorna_riga(p);
        }
        self.aggiorna_riga(indice);
        // Scorri l'elenco solo se il brano non è già a vista (per esempio col "avanti"
        // automatico): se l'hai appena cliccato, l'elenco deve restare fermo.
        if !self.segno.etichette.borrow().contains_key(&(indice as u32)) {
            self.vista
                .scroll_to(indice as u32, gtk::ListScrollFlags::NONE, None);
        }
    }

    fn testo_riga(&self, i: usize) -> String {
        format!("{}. {}", i + 1, self.playlist[i].nome)
    }

    /// Aggiorna il segno ▶ di una riga cambiando solo il testo della sua etichetta.
    /// Non tocchiamo l'elenco: sostituire un elemento farebbe ricreare la riga
    /// alla ListView, che perde il punto in cui eri e fa saltare la lista.
    fn aggiorna_riga(&self, i: usize) {
        let etichetta = self
            .segno
            .etichette
            .borrow()
            .get(&(i as u32))
            .and_then(|w| w.upgrade());
        if let (Some(etichetta), Some(testo)) = (etichetta, self.elenco.string(i as u32)) {
            etichetta.set_label(&con_segno(
                &testo,
                self.segno.corrente.get() == Some(i as u32),
            ));
        }
    }

    /// Aggiunge voci in fondo alla playlist (e al pannello), senza farle partire.
    fn estendi(&mut self, nuove: Vec<Voce>) {
        let prima = self.playlist.len();
        self.playlist.extend(nuove);
        let testi: Vec<String> = (prima..self.playlist.len())
            .map(|i| self.testo_riga(i))
            .collect();
        let testi: Vec<&str> = testi.iter().map(String::as_str).collect();
        self.elenco.splice(prima as u32, 0, &testi);
        self.aggiorna_conteggio();
    }

    fn aggiorna_conteggio(&self) {
        let n = self.playlist.len();
        self.conteggio.set_label(&lingua::brani(n));
    }

    fn sta_suonando(&self) -> bool {
        let (_, stato, _) = self.playbin.state(gst::ClockTime::ZERO);
        stato == gst::State::Playing
    }

    fn play_pausa(&mut self) {
        match self.corrente {
            None if !self.playlist.is_empty() => self.suona(0),
            None => {}
            Some(_) if self.sta_suonando() => {
                let _ = self.playbin.set_state(gst::State::Paused);
                self.pulsante_play
                    .set_icon_name("media-playback-start-symbolic");
            }
            Some(_) => {
                let _ = self.playbin.set_state(gst::State::Playing);
                self.pulsante_play
                    .set_icon_name("media-playback-pause-symbolic");
            }
        }
    }

    fn ferma(&mut self) {
        let _ = self.playbin.set_state(gst::State::Null);
        self.pulsante_play
            .set_icon_name("media-playback-start-symbolic");
        self.barra.set_value(0.0);
        self.tempo.set_label("00:00");
    }

    fn avanti(&mut self) {
        match self.corrente {
            Some(i) if i + 1 < self.playlist.len() => self.suona(i + 1),
            // Fine della playlist: ci fermiamo e ripartiamo dall'inizio al prossimo play.
            Some(i) => {
                self.ferma();
                self.corrente = None;
                self.segno.corrente.set(None);
                self.aggiorna_riga(i);
            }
            None => {}
        }
    }

    /// Come nei vecchi lettori: se il brano è avanti più di 3 secondi ricomincia,
    /// altrimenti torna al brano precedente.
    fn indietro(&mut self) {
        let Some(i) = self.corrente else { return };
        let posizione = self.playbin.query_position::<gst::ClockTime>();
        if i == 0 || posizione.is_some_and(|p| p.seconds() > 3) {
            self.vai_a(0.0);
        } else {
            self.suona(i - 1);
        }
    }

    fn vai_a(&self, secondi: f64) {
        let destinazione = gst::ClockTime::from_mseconds((secondi * 1000.0) as u64);
        let _ = self.playbin.seek_simple(
            gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
            destinazione,
        );
    }

    /// Aggiunge voci in fondo alla playlist e fa partire subito la prima nuova.
    fn aggiungi_e_suona(&mut self, nuove: Vec<Voce>) {
        let prima_nuova = self.playlist.len();
        self.estendi(nuove);
        if prima_nuova < self.playlist.len() {
            self.suona(prima_nuova);
        }
    }

    fn svuota(&mut self) {
        self.ferma();
        self.playlist.clear();
        self.elenco.splice(0, self.elenco.n_items(), &[]);
        self.aggiorna_conteggio();
        self.corrente = None;
        self.segno.corrente.set(None);
        self.titolo.set_label(t(Testo::TrascinaQui));
        self.dettagli.set_label("");
    }

    /// Aggiunge voci in fondo alla playlist; se non stava suonando nulla, parte la prima nuova.
    fn aggiungi(&mut self, nuove: Vec<Voce>) {
        if nuove.is_empty() {
            return;
        }
        let prima_nuova = self.playlist.len();
        self.estendi(nuove);
        if self.corrente.is_none() {
            self.suona(prima_nuova);
        }
    }

    /// I "tag" arrivano dal file o dalla radio: titolo, codec, bitrate.
    /// Le radio mandano così il nome della stazione e il brano in onda.
    fn mostra_tag(&mut self, tag: &gst::TagList) {
        let Some(i) = self.corrente else { return };
        let testo = |t: &str| Some(t.trim().to_string()).filter(|t| !t.is_empty());
        let titolo = tag.get::<gst::tags::Title>().and_then(|t| testo(t.get()));

        if self.qualita.radio {
            if let Some(s) = tag
                .get::<gst::tags::Organization>()
                .and_then(|s| testo(s.get()))
            {
                self.qualita.stazione = Some(s);
            }
            if titolo.is_some() {
                self.qualita.in_onda = titolo;
            }
            // "Stazione — Brano", oppure quello che c'è dei due.
            let riga = match (&self.qualita.stazione, &self.qualita.in_onda) {
                (Some(s), Some(t)) => Some(format!("{s} — {t}")),
                (Some(x), None) | (None, Some(x)) => Some(x.clone()),
                (None, None) => None,
            };
            if let Some(riga) = riga {
                self.titolo.set_label(&format!("{}. {}", i + 1, riga));
            }
        } else if let Some(titolo) = titolo {
            self.titolo.set_label(&format!("{}. {}", i + 1, titolo));
        }
        if let Some(codec) = tag.get::<gst::tags::AudioCodec>() {
            self.qualita.codec = Some(codec.get().to_string());
        }
        let bitrate = tag
            .get::<gst::tags::Bitrate>()
            .or_else(|| tag.get::<gst::tags::NominalBitrate>());
        if let Some(b) = bitrate {
            if b.get() > 0 {
                self.qualita.bitrate = Some(b.get());
            }
        }
    }

    /// Compone la riga dei dettagli: formato, bit, frequenza, canali, bitrate.
    /// Frequenza, canali e bit li leggiamo dall'audio già decodificato.
    fn aggiorna_dettagli(&self, durata: Option<gst::ClockTime>) {
        if self.corrente.is_none() {
            return;
        }
        let q = &self.qualita;
        let mut parti = vec![q.formato()];

        let pad = self
            .playbin
            .emit_by_name::<Option<gst::Pad>>("get-audio-pad", &[&0i32]);
        if let Some(caps) = pad.and_then(|p| p.current_caps()) {
            if let Some(s) = caps.structure(0) {
                if q.senza_perdita() {
                    if let Some(b) = s.get::<&str>("format").ok().and_then(bit) {
                        parti.push(format!("{b} bit"));
                    }
                }
                if let Ok(rate) = s.get::<i32>("rate") {
                    parti.push(khz(rate));
                }
                if let Ok(n) = s.get::<i32>("channels") {
                    parti.push(canali(n));
                }
            }
        }

        // Bitrate: dai tag se c'è, altrimenti stimato da dimensione e durata del file.
        let kbps = q.bitrate.map(|b| b / 1000).or_else(|| {
            let (dim, dur) = (q.dimensione?, durata?.seconds());
            (dur > 0).then(|| (dim * 8 / dur / 1000) as u32)
        });
        if let Some(k) = kbps {
            parti.push(format!("{k} kbps"));
        }

        let riga = parti.join(" · ");
        if self.dettagli.label() != riga {
            #[cfg(debug_assertions)]
            eprintln!("dettagli: {riga}");
            self.dettagli.set_label(&riga);
        }
    }

    /// Chiamata dal timer: aggiorna tempo e barra di avanzamento.
    fn aggiorna_tempo(&self) {
        let Some(posizione) = self.playbin.query_position::<gst::ClockTime>() else {
            return;
        };
        let durata = self.playbin.query_duration::<gst::ClockTime>();
        self.aggiorna_dettagli(durata);
        // Le radio a volte dichiarano una durata finta: per loro vale sempre "LIVE".
        match durata.filter(|_| !self.qualita.radio) {
            Some(durata) => {
                self.barra.set_sensitive(true);
                self.barra.set_range(0.0, durata.seconds() as f64);
                self.barra.set_value(posizione.mseconds() as f64 / 1000.0);
                self.tempo.set_label(&format!(
                    "{} / {}",
                    mm_ss(posizione.seconds()),
                    mm_ss(durata.seconds())
                ));
            }
            // Uno stream radio non ha durata: niente barra, solo il tempo d'ascolto.
            None => {
                self.barra.set_sensitive(false);
                self.barra.set_value(0.0);
                self.tempo
                    .set_label(&format!("{}  ● LIVE", mm_ss(posizione.seconds())));
            }
        }
    }
}

fn mm_ss(secondi: u64) -> String {
    format!("{:02}:{:02}", secondi / 60, secondi % 60)
}

fn estensione(nome: &str) -> String {
    Path::new(nome.split(['?', '#']).next().unwrap_or(nome))
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Nome leggibile di una voce: per i file il nome senza estensione, per gli stream l'indirizzo.
/// Esercizio: leggere i tag dei file (crate `lofty`).
fn nome_da_file(file: &gio::File) -> String {
    match file.path() {
        Some(p) => p
            .file_stem()
            .map(|s| s.to_string_lossy().replace('_', " "))
            .unwrap_or_default(),
        None => file.uri().to_string(),
    }
}

/// Legge una playlist (locale o online) e restituisce le sue voci.
/// I percorsi relativi vengono risolti rispetto alla cartella della playlist.
fn leggi_playlist(file: &gio::File) -> Vec<Voce> {
    let Ok((byte, _)) = file.load_contents(gio::Cancellable::NONE) else {
        eprintln!("Impossibile leggere la playlist {}", file.uri());
        return Vec::new();
    };
    let testo = String::from_utf8_lossy(&byte);
    let base = file.parent();
    let risolvi = |voce: &str| -> gio::File {
        if voce.contains("://") {
            gio::File::for_uri(voce)
        } else if Path::new(voce).is_absolute() {
            gio::File::for_path(voce)
        } else {
            match &base {
                Some(b) => b.resolve_relative_path(voce.replace('\\', "/")),
                None => gio::File::for_path(voce),
            }
        }
    };

    let mut voci = Vec::new();
    if estensione(&file.uri()) == "pls" {
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
            let f = risolvi(&indirizzo);
            let nome = titoli.remove(&n).unwrap_or_else(|| nome_da_file(&f));
            voci.push(Voce {
                uri: f.uri().to_string(),
                nome,
            });
        }
    } else {
        // Formato M3U: una voce per riga; "#EXTINF:durata,Titolo" dà il nome alla riga dopo.
        let mut prossimo_nome: Option<String> = None;
        for riga in testo.lines().map(str::trim) {
            if let Some(info) = riga.strip_prefix("#EXTINF:") {
                prossimo_nome = info.split_once(',').map(|(_, t)| t.trim().to_string());
            } else if !riga.is_empty() && !riga.starts_with('#') {
                let f = risolvi(riga);
                let nome = prossimo_nome.take().unwrap_or_else(|| nome_da_file(&f));
                voci.push(Voce {
                    uri: f.uri().to_string(),
                    nome,
                });
            }
        }
    }
    voci
}

/// Trasforma un file, una cartella o una playlist in voci da suonare.
/// Le cartelle vengono esplorate (sottocartelle comprese) in ordine alfabetico.
fn raccogli(file: &gio::File, risultato: &mut Vec<Voce>) {
    let uri = file.uri();
    let est = estensione(&uri);
    if ESTENSIONI_PLAYLIST.contains(&est.as_str()) {
        risultato.extend(leggi_playlist(file));
        return;
    }
    match file.path() {
        Some(percorso) if percorso.is_dir() => {
            let Ok(voci) = std::fs::read_dir(&percorso) else {
                return;
            };
            let mut figli: Vec<_> = voci.filter_map(|v| v.ok().map(|v| v.path())).collect();
            figli.sort();
            for figlio in figli {
                raccogli(&gio::File::for_path(figlio), risultato);
            }
        }
        Some(_) if !ESTENSIONI_AUDIO.contains(&est.as_str()) => {}
        // File audio locale, oppure indirizzo di uno stream.
        _ => risultato.push(Voce {
            uri: uri.to_string(),
            nome: nome_da_file(file),
        }),
    }
}

fn pulsante(icona: &str, descrizione: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icona);
    b.set_tooltip_text(Some(descrizione));
    b.add_css_class("flat");
    b
}

/// Il pulsante "radio": apre un riquadro dove incollare l'indirizzo di uno stream
/// (o di una playlist online .m3u/.pls).
fn pulsante_radio(stato: &Rc<RefCell<Stato>>) -> gtk::MenuButton {
    let campo = gtk::Entry::new();
    campo.set_placeholder_text(Some(t(Testo::IndirizzoRadio)));
    campo.set_width_chars(32);

    let aggiungi = gtk::Button::with_label(t(Testo::Ascolta));
    aggiungi.add_css_class("suggested-action");

    let riga = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    riga.append(&campo);
    riga.append(&aggiungi);

    let riquadro = gtk::Popover::new();
    riquadro.set_child(Some(&riga));

    let menu = gtk::MenuButton::new();
    menu.set_icon_name("rustabile-radio-symbolic");
    menu.set_tooltip_text(Some(t(Testo::AggiungiRadio)));
    menu.add_css_class("flat");
    menu.set_popover(Some(&riquadro));

    let conferma = {
        let stato = stato.clone();
        let campo = campo.clone();
        let riquadro = riquadro.clone();
        move || {
            let testo = campo.text().trim().to_string();
            if testo.is_empty() {
                return;
            }
            let mut nuove = Vec::new();
            raccogli(&gio::File::for_uri(&testo), &mut nuove);
            // Una radio aggiunta a mano parte subito.
            stato.borrow_mut().aggiungi_e_suona(nuove);
            campo.set_text("");
            riquadro.popdown();
        }
    };
    let c = conferma.clone();
    aggiungi.connect_clicked(move |_| c());
    campo.connect_activate(move |_| conferma());
    menu
}

/// Chi suona adesso nel pannello, e le etichette delle righe a vista.
/// La ListView ricicla poche righe mentre scorri: teniamo traccia di quale
/// etichetta mostra quale posizione, per aggiornare il segno ▶ senza toccare l'elenco.
#[derive(Default)]
struct Segno {
    corrente: std::cell::Cell<Option<u32>>,
    etichette: RefCell<std::collections::HashMap<u32, glib::WeakRef<gtk::Label>>>,
}

fn con_segno(testo: &str, corrente: bool) -> String {
    if corrente {
        format!("▶ {testo}")
    } else {
        format!("   {testo}")
    }
}

/// La vista dell'elenco: una `ListView` che mostra i testi di `elenco`.
/// Crea solo le righe visibili, quindi regge anche playlist di migliaia di radio.
fn crea_vista_playlist(elenco: &gtk::StringList, segno: &Rc<Segno>) -> gtk::ListView {
    let fabbrica = gtk::SignalListItemFactory::new();
    fabbrica.connect_setup(|_, oggetto| {
        let riga = oggetto.downcast_ref::<gtk::ListItem>().expect("ListItem");
        let etichetta = gtk::Label::new(None);
        etichetta.set_xalign(0.0);
        etichetta.set_ellipsize(gtk::pango::EllipsizeMode::End);
        etichetta.set_margin_start(6);
        etichetta.set_margin_end(6);
        riga.set_child(Some(&etichetta));
    });
    let s = segno.clone();
    fabbrica.connect_bind(move |_, oggetto| {
        let riga = oggetto.downcast_ref::<gtk::ListItem>().expect("ListItem");
        let (Some(testo), Some(etichetta)) = (
            riga.item().and_downcast::<gtk::StringObject>(),
            riga.child().and_downcast::<gtk::Label>(),
        ) else {
            return;
        };
        let posizione = riga.position();
        etichetta.set_label(&con_segno(
            &testo.string(),
            s.corrente.get() == Some(posizione),
        ));
        s.etichette
            .borrow_mut()
            .insert(posizione, etichetta.downgrade());
    });
    let s = segno.clone();
    fabbrica.connect_unbind(move |_, oggetto| {
        let riga = oggetto.downcast_ref::<gtk::ListItem>().expect("ListItem");
        if let Some(etichetta) = riga.child().and_downcast::<gtk::Label>() {
            s.etichette
                .borrow_mut()
                .retain(|_, w| w.upgrade().as_ref() != Some(&etichetta));
        }
    });
    let vista = gtk::ListView::new(
        Some(gtk::NoSelection::new(Some(elenco.clone()))),
        Some(fabbrica),
    );
    vista.set_single_click_activate(true);
    vista
}

/// Il pannello playlist sotto i comandi, come nei lettori di una volta:
/// l'elenco dei brani (un clic per farne partire uno), il conteggio e "Svuota".
fn pannello_playlist(stato: &Rc<RefCell<Stato>>) -> gtk::Box {
    let (vista, conteggio) = {
        let s = stato.borrow();
        (s.vista.clone(), s.conteggio.clone())
    };

    let st = stato.clone();
    vista.connect_activate(move |_, posizione| st.borrow_mut().suona(posizione as usize));

    let scorrimento = gtk::ScrolledWindow::new();
    scorrimento.set_child(Some(&vista));
    scorrimento.set_hscrollbar_policy(gtk::PolicyType::Never);
    scorrimento.set_min_content_height(220);
    scorrimento.set_vexpand(true);
    scorrimento.set_has_frame(true);

    let svuota = gtk::Button::with_label(t(Testo::Svuota));
    svuota.add_css_class("flat");
    let st = stato.clone();
    svuota.connect_clicked(move |_| st.borrow_mut().svuota());

    conteggio.set_hexpand(true);
    conteggio.set_xalign(0.0);
    conteggio.add_css_class("dim-label");
    conteggio.add_css_class("caption");

    let piede = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    piede.append(&conteggio);
    piede.append(&svuota);

    let pannello = gtk::Box::new(gtk::Orientation::Vertical, 4);
    pannello.set_margin_top(6);
    pannello.append(&scorrimento);
    pannello.append(&piede);
    pannello
}

/// Finestrella delle preferenze: due interruttori, salvati subito su file.
fn mostra_preferenze(finestra: &gtk::ApplicationWindow, stato: &Rc<RefCell<Stato>>) {
    let riga = |titolo: &str, spiegazione: &str, attivo: bool| {
        let testi = gtk::Box::new(gtk::Orientation::Vertical, 2);
        testi.set_hexpand(true);
        let t = gtk::Label::new(Some(titolo));
        t.set_xalign(0.0);
        let s = gtk::Label::new(Some(spiegazione));
        s.set_xalign(0.0);
        s.set_wrap(true);
        s.set_max_width_chars(40);
        s.add_css_class("dim-label");
        s.add_css_class("caption");
        testi.append(&t);
        testi.append(&s);
        let interruttore = gtk::Switch::new();
        interruttore.set_active(attivo);
        interruttore.set_valign(gtk::Align::Center);
        let r = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        r.append(&testi);
        r.append(&interruttore);
        (r, interruttore)
    };

    let imp = &stato.borrow().impostazioni;
    let (r1, sopra) = riga(
        t(Testo::SempreSopra),
        t(Testo::SempreSopraSpiegazione),
        imp.sempre_sopra,
    );
    let (r2, trascina) = riga(
        t(Testo::TrascinaSuona),
        t(Testo::TrascinaSuonaSpiegazione),
        imp.trascina_suona,
    );

    let colonna = gtk::Box::new(gtk::Orientation::Vertical, 18);
    colonna.set_margin_top(18);
    colonna.set_margin_bottom(18);
    colonna.set_margin_start(18);
    colonna.set_margin_end(18);
    colonna.append(&r1);
    colonna.append(&r2);
    colonna.append(&riga_lingua(stato));

    let s = stato.clone();
    sopra.connect_active_notify(move |i| {
        let mut stato = s.borrow_mut();
        stato.impostazioni.sempre_sopra = i.is_active();
        stato.impostazioni.salva();
    });
    let s = stato.clone();
    trascina.connect_active_notify(move |i| {
        let mut stato = s.borrow_mut();
        stato.impostazioni.trascina_suona = i.is_active();
        stato.impostazioni.salva();
    });

    gtk::Window::builder()
        .title(t(Testo::Preferenze))
        .transient_for(finestra)
        .modal(true)
        .resizable(false)
        .child(&colonna)
        .build()
        .present();
}

/// Scelta della lingua: "Automatica" segue il desktop. Vale dalla prossima apertura.
fn riga_lingua(stato: &Rc<RefCell<Stato>>) -> gtk::Box {
    const CODICI: [&str; 6] = ["auto", "it", "en", "de", "es", "fr"];
    let nomi = [
        t(Testo::LinguaAutomatica),
        "Italiano",
        "English",
        "Deutsch",
        "Español",
        "Français",
    ];
    let scelta = gtk::DropDown::from_strings(&nomi);
    let attuale = stato.borrow().impostazioni.lingua.clone();
    let indice = CODICI.iter().position(|c| *c == attuale).unwrap_or(0);
    scelta.set_selected(indice as u32);
    scelta.set_valign(gtk::Align::Center);

    let s = stato.clone();
    scelta.connect_selected_notify(move |d| {
        let mut stato = s.borrow_mut();
        stato.impostazioni.lingua = CODICI[d.selected() as usize].to_string();
        stato.impostazioni.salva();
    });

    let testi = gtk::Box::new(gtk::Orientation::Vertical, 2);
    testi.set_hexpand(true);
    let titolo = gtk::Label::new(Some(t(Testo::TitoloLingua)));
    titolo.set_xalign(0.0);
    let nota = gtk::Label::new(Some(t(Testo::LinguaSpiegazione)));
    nota.set_xalign(0.0);
    nota.add_css_class("dim-label");
    nota.add_css_class("caption");
    testi.append(&titolo);
    testi.append(&nota);

    let riga = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    riga.append(&testi);
    riga.append(&scelta);
    riga
}

fn mostra_informazioni(finestra: &gtk::ApplicationWindow) {
    gtk::AboutDialog::builder()
        .transient_for(finestra)
        .modal(true)
        .program_name("Rustabile")
        .version(env!("CARGO_PKG_VERSION"))
        .comments(t(Testo::Descrizione))
        .logo_icon_name("audio-x-generic")
        .authors(["andreae75"])
        .license_type(gtk::License::MitX11)
        .build()
        .present();
}

/// Il pulsante con il menu: Preferenze e Informazioni.
fn pulsante_menu(finestra: &gtk::ApplicationWindow, stato: &Rc<RefCell<Stato>>) -> gtk::MenuButton {
    let preferenze = gio::SimpleAction::new("preferenze", None);
    let (w, s) = (finestra.clone(), stato.clone());
    preferenze.connect_activate(move |_, _| mostra_preferenze(&w, &s));
    finestra.add_action(&preferenze);

    let informazioni = gio::SimpleAction::new("informazioni", None);
    let w = finestra.clone();
    informazioni.connect_activate(move |_, _| mostra_informazioni(&w));
    finestra.add_action(&informazioni);

    let modello = gio::Menu::new();
    modello.append(Some(t(Testo::Preferenze)), Some("win.preferenze"));
    modello.append(Some(t(Testo::Informazioni)), Some("win.informazioni"));

    let menu = gtk::MenuButton::new();
    menu.set_icon_name("open-menu-symbolic");
    menu.set_tooltip_text(Some(t(Testo::Menu)));
    menu.add_css_class("flat");
    menu.set_menu_model(Some(&modello));
    menu
}

/// "Sempre in primo piano". Su Wayland un'app non può chiederlo, quindi quando
/// l'opzione è attiva Rustabile parte con Xwayland (vedi `main`) e qui manda
/// al window manager la richiesta X11 standard `_NET_WM_STATE_ABOVE`.
fn metti_sopra(finestra: &gtk::ApplicationWindow) {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ClientMessageEvent, ConnectionExt, EventMask};

    let Some(superficie) = finestra.surface() else {
        return;
    };
    let Ok(superficie_x11) = superficie.downcast::<gdk_x11::X11Surface>() else {
        eprintln!("Sempre in primo piano: la finestra non usa X11, ignoro");
        return;
    };
    let xid = superficie_x11.xid() as u32;

    let Ok((conn, schermo)) = x11rb::connect(None) else {
        return;
    };
    let radice = conn.setup().roots[schermo].root;
    let atomo = |nome: &str| {
        conn.intern_atom(false, nome.as_bytes())
            .ok()?
            .reply()
            .ok()
            .map(|r| r.atom)
    };
    let (Some(stato), Some(sopra)) = (atomo("_NET_WM_STATE"), atomo("_NET_WM_STATE_ABOVE")) else {
        return;
    };
    // data: [azione 1 = aggiungi, proprietà, seconda proprietà, sorgente 1 = applicazione, 0]
    let evento = ClientMessageEvent::new(32, xid, stato, [1u32, sopra, 0, 1, 0]);
    let _ = conn.send_event(
        false,
        radice,
        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        evento,
    );
    let _ = conn.flush();
}

/// Testo trascinato (per esempio un link da un browser): una voce per riga.
fn file_da_testo(testo: &str) -> Vec<gio::File> {
    testo
        .lines()
        .map(str::trim)
        .filter(|r| !r.is_empty() && !r.starts_with('#'))
        .map(|r| {
            if r.contains("://") {
                gio::File::for_uri(r)
            } else {
                gio::File::for_path(r)
            }
        })
        .collect()
}

/// La finestra e il suo stato, creati una volta sola e riusati da "activate" e "open".
type Finestra = (gtk::ApplicationWindow, Rc<RefCell<Stato>>);

fn costruisci_finestra(app: &gtk::Application) -> Finestra {
    // --- Widget -----------------------------------------------------------
    let titolo = gtk::Label::new(Some(t(Testo::TrascinaQui)));
    titolo.set_ellipsize(gtk::pango::EllipsizeMode::End);
    titolo.set_xalign(0.0);

    let tempo = gtk::Label::new(Some("00:00"));
    tempo.add_css_class("tempo");
    tempo.set_xalign(0.0);

    let dettagli = gtk::Label::new(None);
    dettagli.add_css_class("dettagli");
    dettagli.set_ellipsize(gtk::pango::EllipsizeMode::End);
    dettagli.set_xalign(0.0);

    let display = gtk::Box::new(gtk::Orientation::Vertical, 2);
    display.add_css_class("display");
    display.append(&tempo);
    display.append(&titolo);
    display.append(&dettagli);

    let barra = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 1.0);
    barra.set_draw_value(false);

    let indietro = pulsante("media-skip-backward-symbolic", t(Testo::Indietro));
    let pulsante_play = pulsante("media-playback-start-symbolic", t(Testo::PlayPausa));
    let ferma = pulsante("media-playback-stop-symbolic", t(Testo::Stop));
    let avanti = pulsante("media-skip-forward-symbolic", t(Testo::Avanti));

    let volume = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.05);
    volume.set_draw_value(false);
    let impostazioni = Impostazioni::carica();
    volume.set_value(impostazioni.volume);
    volume.set_hexpand(true);
    volume.set_tooltip_text(Some(t(Testo::Volume)));

    // --- Motore audio ---------------------------------------------------------
    let playbin = gst::ElementFactory::make("playbin")
        .build()
        .expect("GStreamer: manca l'elemento playbin");
    playbin.set_property("volume", volume.value());

    let elenco = gtk::StringList::new(&[]);
    let segno = Rc::new(Segno::default());
    let stato = Rc::new(RefCell::new(Stato {
        playbin: playbin.clone(),
        playlist: Vec::new(),
        corrente: None,
        titolo,
        tempo,
        barra: barra.clone(),
        pulsante_play: pulsante_play.clone(),
        dettagli,
        qualita: Qualita::default(),
        impostazioni,
        elenco: elenco.clone(),
        vista: crea_vista_playlist(&elenco, &segno),
        segno: segno.clone(),
        conteggio: gtk::Label::new(None),
        _guardia_bus: None,
    }));
    stato.borrow().aggiorna_conteggio();

    // Pannello playlist sotto i comandi, che si apre e chiude con un pulsante.
    let mostra_playlist = gtk::ToggleButton::new();
    mostra_playlist.set_icon_name("view-list-symbolic");
    mostra_playlist.set_tooltip_text(Some(t(Testo::MostraPlaylist)));
    mostra_playlist.add_css_class("flat");
    let tendina = gtk::Revealer::new();
    tendina.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    tendina.set_child(Some(&pannello_playlist(&stato)));

    let comandi = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    for b in [&indietro, &pulsante_play, &ferma, &avanti] {
        comandi.append(b);
    }
    comandi.append(&mostra_playlist);
    comandi.append(&pulsante_radio(&stato));
    comandi.append(&gtk::Image::from_icon_name("audio-volume-medium-symbolic"));
    comandi.append(&volume);

    let contenuto = gtk::Box::new(gtk::Orientation::Vertical, 6);
    contenuto.set_margin_top(8);
    contenuto.set_margin_bottom(8);
    contenuto.set_margin_start(8);
    contenuto.set_margin_end(8);
    contenuto.append(&display);
    contenuto.append(&barra);
    contenuto.append(&comandi);
    contenuto.append(&tendina);

    let finestra = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Rustabile")
        .default_width(380)
        .child(&contenuto)
        .build();
    comandi.append(&pulsante_menu(&finestra, &stato));

    // Aprendo il pannello la finestra si allunga; chiudendolo torna compatta.
    let (t, w, s) = (tendina.clone(), finestra.clone(), stato.clone());
    mostra_playlist.connect_toggled(move |b| {
        let aperta = b.is_active();
        t.set_vexpand(aperta);
        t.set_reveal_child(aperta);
        let mut stato = s.borrow_mut();
        stato.impostazioni.mostra_playlist = aperta;
        stato.impostazioni.salva();
        if !aperta {
            let w = w.clone();
            glib::timeout_add_local_once(Duration::from_millis(260), move || {
                w.set_default_size(w.width(), 1);
            });
        }
    });
    if stato.borrow().impostazioni.mostra_playlist {
        mostra_playlist.set_active(true);
    }

    // Sempre in primo piano: la richiesta va fatta quando la finestra è sullo schermo.
    if stato.borrow().impostazioni.sempre_sopra {
        finestra.connect_map(|w| {
            let w = w.clone();
            glib::idle_add_local_once(move || metti_sopra(&w));
        });
    }

    // Messaggi di GStreamer: a fine brano si passa al successivo, i tag aggiornano il titolo.
    let bus = playbin.bus().expect("playbin senza bus");
    let s = stato.clone();
    let guardia = bus
        .add_watch_local(move |_, messaggio| {
            match messaggio.view() {
                gst::MessageView::Eos(..) => s.borrow_mut().avanti(),
                gst::MessageView::Error(errore) => {
                    eprintln!("Errore di riproduzione: {}", errore.error());
                    s.borrow_mut().avanti();
                }
                gst::MessageView::Tag(tag) => s.borrow_mut().mostra_tag(&tag.tags()),
                _ => {}
            }
            glib::ControlFlow::Continue
        })
        .expect("impossibile ascoltare il bus di GStreamer");
    stato.borrow_mut()._guardia_bus = Some(guardia);

    // --- Pulsanti -------------------------------------------------------------
    let s = stato.clone();
    pulsante_play.connect_clicked(move |_| s.borrow_mut().play_pausa());
    let s = stato.clone();
    ferma.connect_clicked(move |_| s.borrow_mut().ferma());
    let s = stato.clone();
    avanti.connect_clicked(move |_| s.borrow_mut().avanti());
    let s = stato.clone();
    indietro.connect_clicked(move |_| s.borrow_mut().indietro());

    let p = playbin.clone();
    volume.connect_value_changed(move |v| p.set_property("volume", v.value()));

    // `change-value` scatta solo quando è l'utente a spostare la barra,
    // non quando la aggiorniamo noi dal timer.
    let s = stato.clone();
    barra.connect_change_value(move |_, _, valore| {
        s.borrow().vai_a(valore);
        glib::Propagation::Proceed
    });

    // --- Trascinamento di file, cartelle, playlist e link ---------------------
    // Accettiamo sia elenchi di file (Nautilus) sia testo (un link da un browser).
    let destinazione = gtk::DropTarget::new(glib::Type::INVALID, gdk::DragAction::COPY);
    destinazione.set_types(&[gdk::FileList::static_type(), String::static_type()]);
    let s = stato.clone();
    destinazione.connect_drop(move |_, valore, _, _| {
        let file = if let Ok(lista) = valore.get::<gdk::FileList>() {
            lista.files()
        } else if let Ok(testo) = valore.get::<String>() {
            file_da_testo(&testo)
        } else {
            return false;
        };
        let mut nuove = Vec::new();
        for f in &file {
            raccogli(f, &mut nuove);
        }
        if nuove.is_empty() {
            return false;
        }
        let mut stato = s.borrow_mut();
        if stato.impostazioni.trascina_suona {
            stato.aggiungi_e_suona(nuove);
        } else {
            stato.aggiungi(nuove);
        }
        true
    });
    finestra.add_controller(destinazione);

    // --- Timer: aggiorna tempo e barra due volte al secondo -------------------
    let s = stato.clone();
    glib::timeout_add_local(Duration::from_millis(500), move || {
        s.borrow().aggiorna_tempo();
        glib::ControlFlow::Continue
    });

    // Alla chiusura fermiamo l'audio e ricordiamo il volume.
    let s = stato.clone();
    finestra.connect_close_request(move |_| {
        let _ = playbin.set_state(gst::State::Null);
        let mut stato = s.borrow_mut();
        stato.impostazioni.volume = volume.value();
        stato.impostazioni.salva();
        glib::Propagation::Proceed
    });

    (finestra, stato)
}

fn carica_stile() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("stile.css"));
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        registra_icone(&display);
    }
}

/// Le icone nostre (la radio) sono dentro il programma: all'avvio le scriviamo
/// nella cache e diciamo al tema di icone di cercarle lì. Così funzionano
/// ovunque sia installato Rustabile, e GTK le colora come le altre icone simboliche.
fn registra_icone(display: &gdk::Display) {
    let base = glib::user_cache_dir().join("rustabile").join("icone");
    let cartella = base.join("hicolor").join("scalable").join("actions");
    let file = cartella.join("rustabile-radio-symbolic.svg");
    let contenuto = include_str!("../data/icons/rustabile-radio-symbolic.svg");
    if std::fs::read_to_string(&file).ok().as_deref() != Some(contenuto) {
        let _ = std::fs::create_dir_all(&cartella);
        let _ = std::fs::write(&file, contenuto);
    }
    gtk::IconTheme::for_display(display).add_search_path(&base);
}

fn main() -> glib::ExitCode {
    // "Sempre in primo piano" richiede X11: lo scegliamo prima che GTK si avvii.
    // SAFETY: siamo all'inizio di main, nessun altro thread legge l'ambiente.
    if Impostazioni::carica().sempre_sopra {
        unsafe { std::env::set_var("GDK_BACKEND", "x11") };
    }

    gst::init().expect("impossibile inizializzare GStreamer");

    // HANDLES_OPEN: accettiamo file da riga di comando e da "Apri con" di Nautilus.
    // GApplication garantisce una sola istanza: se Rustabile è già aperto, i file
    // aperti dopo arrivano alla finestra esistente invece di aprirne un'altra.
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();
    app.connect_startup(|_| carica_stile());

    let finestra: Rc<RefCell<Option<Finestra>>> = Rc::new(RefCell::new(None));
    let ottieni = {
        let finestra = finestra.clone();
        move |app: &gtk::Application| -> Finestra {
            let mut f = finestra.borrow_mut();
            let (w, s) = f.get_or_insert_with(|| costruisci_finestra(app));
            (w.clone(), s.clone())
        }
    };

    let o = ottieni.clone();
    app.connect_activate(move |app| o(app).0.present());

    // Aprire file da fuori (doppio clic in Nautilus): li accodiamo e suona subito il primo.
    app.connect_open(move |app, file, _| {
        let (w, stato) = ottieni(app);
        let mut nuove = Vec::new();
        for f in file {
            raccogli(f, &mut nuove);
        }
        stato.borrow_mut().aggiungi_e_suona(nuove);
        w.present();
    });

    app.run()
}

#[cfg(test)]
mod test {
    use super::*;

    fn cartella_di_prova(nome: &str) -> std::path::PathBuf {
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

    #[test]
    fn cartella_in_ordine_senza_file_non_audio() {
        let d = cartella_di_prova("cartella");
        let mut voci = Vec::new();
        raccogli(&gio::File::for_path(d.join("album")), &mut voci);
        let nomi: Vec<_> = voci.iter().map(|v| v.nome.as_str()).collect();
        assert_eq!(nomi, ["01 - Uno", "02 - Due"]);
    }

    #[test]
    fn m3u_con_titoli_percorsi_relativi_e_radio() {
        let d = cartella_di_prova("m3u");
        std::fs::write(
            d.join("lista.m3u"),
            "#EXTM3U\n#EXTINF:123,Artista - Primo\nalbum/01 - Uno.mp3\n\nalbum/02 - Due.flac\n#EXTINF:-1,Radio di prova\nhttp://radio.example/stream\n",
        )
        .unwrap();
        let mut voci = Vec::new();
        raccogli(&gio::File::for_path(d.join("lista.m3u")), &mut voci);
        assert_eq!(voci.len(), 3);
        assert_eq!(voci[0].nome, "Artista - Primo");
        assert!(voci[0].uri.ends_with("album/01%20-%20Uno.mp3"));
        assert_eq!(voci[1].nome, "02 - Due");
        assert_eq!(voci[2].nome, "Radio di prova");
        assert_eq!(voci[2].uri, "http://radio.example/stream");
    }

    #[test]
    fn pls_con_titoli() {
        let d = cartella_di_prova("pls");
        std::fs::write(
            d.join("radio.pls"),
            "[playlist]\nFile1=http://radio.example/a\nTitle1=Radio A\nFile2=http://radio.example/b\nNumberOfEntries=2\n",
        )
        .unwrap();
        let mut voci = Vec::new();
        raccogli(&gio::File::for_path(d.join("radio.pls")), &mut voci);
        assert_eq!(voci.len(), 2);
        assert_eq!(voci[0].nome, "Radio A");
        assert_eq!(voci[1].uri, "http://radio.example/b");
    }
}

#[cfg(test)]
mod test_dettagli {
    use super::*;

    #[test]
    fn frequenze_bit_e_nomi() {
        assert_eq!(khz(44100), "44,1 kHz");
        assert_eq!(khz(96000), "96 kHz");
        assert_eq!(bit("S16LE"), Some(16));
        assert_eq!(bit("S24_32LE"), Some(24));
        assert_eq!(bit("F32LE"), None);
        let q = Qualita {
            codec: Some("Free Lossless Audio Codec (FLAC)".into()),
            ..Default::default()
        };
        assert_eq!(q.formato(), "FLAC");
        assert!(q.senza_perdita());
    }
}
