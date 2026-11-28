//! Rustabile: un mini lettore musicale per GNOME, in Rust.
//!
//! GTK4 per la finestra, GStreamer (`playbin`) per l'audio. Playlist, coda,
//! preferenze e lingue stanno in `nucleo`, comune alla versione WinUI.
//! - Trascina file, cartelle o playlist (.m3u, .m3u8, .pls) nella finestra.
//! - Il pulsante "radio" accetta l'indirizzo di uno stream o di una playlist online.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gst::prelude::*;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use rustabile_nucleo::voci;
use rustabile_nucleo::{Coda, Impostazioni, Qualita, Testo, Voce, lingua, mm_ss, t};

const APP_ID: &str = "io.github.andreae75.Rustabile";

/// Tutto lo stato del lettore: il motore audio, la playlist e i widget da aggiornare.
///
/// Sta dentro un `Rc<RefCell<Stato>>` perché lo condividono tutte le callback
/// (pulsanti, timer, messaggi di GStreamer): `Rc` per avere più proprietari,
/// `RefCell` per poterlo modificare anche se la callback ne ha solo un riferimento.
struct Stato {
    playbin: gst::Element,
    coda: Coda,
    titolo: gtk::Label,
    tempo: gtk::Label,
    barra: gtk::Scale,
    pulsante_play: gtk::Button,
    volume: gtk::Scale,
    /// Riga piccola con formato e qualità: "FLAC · 16 bit · 44,1 kHz · stereo · 912 kbps".
    dettagli: gtk::Label,
    qualita: Qualita,
    impostazioni: Impostazioni,
    /// Il pannello playlist: un elenco di testi ("▶ 3. Titolo") mostrato da una ListView.
    elenco: gtk::StringList,
    vista: gtk::ListView,
    segno: Rc<Segno>,
    /// Fino a quando la riga piccola mostra un avviso invece dei dettagli.
    avviso_fino: Option<std::time::Instant>,
    conteggio: gtk::Label,
    /// Finché esiste, GStreamer ci consegna i suoi messaggi (fine brano, errori, tag).
    _guardia_bus: Option<gst::bus::BusWatchGuard>,
}

/// L'URI che usa il nucleo: se GIO conosce un percorso (anche di rete, via gvfs)
/// vale quello, altrimenti l'indirizzo così com'è.
fn uri_di(file: &gio::File) -> String {
    match file.path() {
        Some(p) => voci::uri_da_percorso(&p),
        None => file.uri().to_string(),
    }
}

/// File, cartelle, playlist e radio diventano voci. Le playlist online si leggono con GIO.
fn raccogli(uri: &str, risultato: &mut Vec<Voce>) {
    let leggi = |u: &str| {
        let (byte, _) = gio::File::for_uri(u)
            .load_contents(gio::Cancellable::NONE)
            .ok()?;
        Some(byte.to_vec())
    };
    voci::raccogli(uri, &leggi, risultato);
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
        let Some(voce) = self.coda.voci.get(indice) else {
            return;
        };
        let _ = self.playbin.set_state(gst::State::Null);
        self.playbin.set_property("uri", voce.uri.as_str());
        let _ = self.playbin.set_state(gst::State::Playing);

        self.qualita = Qualita::per(&voce.uri);
        self.dettagli.set_label("");
        // Con una radio non si può andare avanti e indietro: barra grigia da subito.
        self.barra.set_sensitive(!self.qualita.radio);
        self.barra.set_value(0.0);

        let precedente = self.coda.corrente.replace(indice);
        self.titolo.set_label(&self.coda.testo_riga(indice));
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
        let prima = self.coda.voci.len();
        self.coda.voci.extend(nuove);
        let testi: Vec<String> = (prima..self.coda.voci.len())
            .map(|i| self.coda.testo_riga(i))
            .collect();
        let testi: Vec<&str> = testi.iter().map(String::as_str).collect();
        self.elenco.splice(prima as u32, 0, &testi);
        self.aggiorna_conteggio();
    }

    fn aggiorna_conteggio(&self) {
        let n = self.coda.voci.len();
        self.conteggio.set_label(&lingua::brani(n));
    }

    /// Alza o abbassa il volume; il cursore aggiorna da solo il motore audio.
    fn cambia_volume(&mut self, di: f64) {
        self.volume
            .set_value((self.volume.value() + di).clamp(0.0, 1.0));
    }

    fn sta_suonando(&self) -> bool {
        let (_, stato, _) = self.playbin.state(gst::ClockTime::ZERO);
        stato == gst::State::Playing
    }

    fn play_pausa(&mut self) {
        match self.coda.corrente {
            None if !self.coda.voci.is_empty() => self.suona(0),
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
        if let Some(prossima) = self.coda.successiva() {
            self.suona(prossima);
        } else if let Some(i) = self.coda.corrente {
            // Fine della playlist: ci fermiamo e ripartiamo dall'inizio al prossimo play.
            self.ferma();
            self.coda.corrente = None;
            self.segno.corrente.set(None);
            self.aggiorna_riga(i);
        }
    }

    /// Come nei vecchi lettori: se il brano è avanti più di 3 secondi ricomincia,
    /// altrimenti torna al brano precedente.
    fn indietro(&mut self) {
        let Some(i) = self.coda.corrente else {
            return;
        };
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
        let prima_nuova = self.coda.voci.len();
        self.estendi(nuove);
        if prima_nuova < self.coda.voci.len() {
            self.suona(prima_nuova);
        }
    }

    /// Sposta nel Cestino il file del brano in corso (si può recuperare dal Cestino),
    /// lo toglie dalla playlist e passa al brano successivo.
    /// Una radio non è un file: la togliamo solo dalla playlist.
    fn cestina_corrente(&mut self) {
        let Some(i) = self.coda.corrente else {
            return;
        };
        let Voce { uri, nome, origine } = self.coda.voci[i].clone();
        let file = gio::File::for_uri(&uri);

        // Fermiamo la riproduzione prima, così il file non è più in uso.
        let _ = self.playbin.set_state(gst::State::Null);
        if file.path().is_some() {
            if let Err(errore) = file.trash(gio::Cancellable::NONE) {
                // Non spostato (per esempio un disco senza Cestino): riprende a suonare.
                let _ = self.playbin.set_state(gst::State::Playing);
                self.avviso(&format!("{} {errore}", t(Testo::ErroreCestino)));
                return;
            }
        }

        // Il brano arrivava da una playlist sul disco? Togliamo anche la sua riga,
        // se il file della playlist è scrivibile (altrimenti resta com'è).
        let mut da_dove = String::new();
        if let (true, Some(origine)) = (file.path().is_some(), &origine) {
            if voci::togli_da_playlist(origine, &uri) {
                if let Some(nome_lista) = origine.file_name() {
                    da_dove = format!(" — {}", nome_lista.to_string_lossy());
                }
            }
        }

        self.coda.togli_corrente();
        self.segno.corrente.set(None);
        // Le righe dopo quella tolta cambiano numero: le riscriviamo tutte.
        let testi: Vec<String> = (i..self.coda.voci.len())
            .map(|j| self.coda.testo_riga(j))
            .collect();
        let testi: Vec<&str> = testi.iter().map(String::as_str).collect();
        self.elenco
            .splice(i as u32, self.elenco.n_items() - i as u32, &testi);
        self.aggiorna_conteggio();

        if i < self.coda.voci.len() {
            self.suona(i);
        } else {
            self.ferma();
            self.titolo.set_label(t(Testo::TrascinaQui));
        }
        self.avviso(&format!("{} {nome}{da_dove}", t(Testo::SpostatoNelCestino)));
    }

    /// Salva la playlist come M3U: percorsi completi per i file, indirizzi per le radio.
    fn salva_playlist(&mut self, file: &gio::File) {
        let testo = self.coda.m3u();
        let esito = match file.path() {
            Some(p) => std::fs::write(&p, testo).map_err(|e| e.to_string()),
            None => Err("percorso non locale".into()),
        };
        match esito {
            Ok(()) => self.avviso(&format!(
                "{} {}",
                t(Testo::PlaylistSalvata),
                file.basename()
                    .map(|b| b.display().to_string())
                    .unwrap_or_default()
            )),
            Err(e) => self.avviso(&format!("{} {e}", t(Testo::ErroreSalvataggio))),
        }
    }

    /// Un messaggio nella riga piccola, che resta per qualche secondo
    /// prima che torni la riga con formato e qualità.
    fn avviso(&mut self, testo: &str) {
        self.dettagli.set_label(testo);
        self.avviso_fino = Some(std::time::Instant::now() + Duration::from_secs(4));
    }

    fn svuota(&mut self) {
        self.ferma();
        self.coda.svuota();
        self.elenco.splice(0, self.elenco.n_items(), &[]);
        self.aggiorna_conteggio();
        self.segno.corrente.set(None);
        self.titolo.set_label(t(Testo::TrascinaQui));
        self.dettagli.set_label("");
    }

    /// Aggiunge voci in fondo alla playlist; se non stava suonando nulla, parte la prima nuova.
    fn aggiungi(&mut self, nuove: Vec<Voce>) {
        if nuove.is_empty() {
            return;
        }
        let prima_nuova = self.coda.voci.len();
        self.estendi(nuove);
        if self.coda.corrente.is_none() {
            self.suona(prima_nuova);
        }
    }

    /// I "tag" arrivano dal file o dalla radio: titolo, codec, bitrate.
    /// Le radio mandano così il nome della stazione e il brano in onda.
    fn mostra_tag(&mut self, tag: &gst::TagList) {
        let Some(i) = self.coda.corrente else {
            return;
        };
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
            if let Some(riga) = self.qualita.riga_radio() {
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
        if self.coda.corrente.is_none()
            || self
                .avviso_fino
                .is_some_and(|fino| std::time::Instant::now() < fino)
        {
            return;
        }
        let pad = self
            .playbin
            .emit_by_name::<Option<gst::Pad>>("get-audio-pad", &[&0i32]);
        let caps = pad.and_then(|p| p.current_caps());
        let s = caps.as_ref().and_then(|c| c.structure(0));
        let riga = self.qualita.dettagli(
            s.and_then(|s| s.get::<&str>("format").ok()).and_then(bit),
            s.and_then(|s| s.get::<i32>("rate").ok()),
            s.and_then(|s| s.get::<i32>("channels").ok()),
            durata.map(|d| d.seconds()),
        );
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
            raccogli(&uri_di(&gio::File::for_uri(&testo)), &mut nuove);
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
    colonna.append(&r2);
    colonna.append(&riga_lingua(stato));

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
        .logo_icon_name(APP_ID)
        .authors(["andreae75"])
        .license_type(gtk::License::MitX11)
        .website(env!("CARGO_PKG_REPOSITORY"))
        .build()
        .present();
}

/// Vero se in questa finestra si sta scrivendo in un campo di testo:
/// allora i tasti (Spazio, Canc, lettere) servono al testo, non al lettore.
fn sta_scrivendo(finestra: &gtk::ApplicationWindow) -> bool {
    gtk::prelude::GtkWindowExt::focus(finestra)
        .is_some_and(|f| f.type_().is_a(gtk::Editable::static_type()))
}

/// Le azioni del lettore (play/pausa, stop, avanti, indietro, volume),
/// a cui `main` collega le scorciatoie da tastiera.
fn azioni_lettore(finestra: &gtk::ApplicationWindow, stato: &Rc<RefCell<Stato>>) {
    let azioni: [(&str, fn(&mut Stato)); 6] = [
        ("play_pausa", Stato::play_pausa),
        ("ferma", Stato::ferma),
        ("avanti", Stato::avanti),
        ("indietro", Stato::indietro),
        ("volume_su", |s| s.cambia_volume(0.05)),
        ("volume_giu", |s| s.cambia_volume(-0.05)),
    ];
    for (nome, fai) in azioni {
        let azione = gio::SimpleAction::new(nome, None);
        let (w, s) = (finestra.clone(), stato.clone());
        azione.connect_activate(move |_, _| {
            if !sta_scrivendo(&w) {
                fai(&mut s.borrow_mut());
            }
        });
        finestra.add_action(&azione);
    }
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

    // Sposta nel Cestino il brano in corso: anche col tasto Canc (vedi `main`).
    let cestina = gio::SimpleAction::new("cestina", None);
    let (w, s) = (finestra.clone(), stato.clone());
    cestina.connect_activate(move |_, _| {
        // Se stai scrivendo (per esempio l'indirizzo di una radio), Canc cancella
        // il testo e basta: non deve mai finire un brano nel Cestino per sbaglio.
        if !sta_scrivendo(&w) {
            s.borrow_mut().cestina_corrente();
        }
    });
    finestra.add_action(&cestina);

    // Apri cartella… e Salva playlist…: finestre di scelta file di GNOME.
    let apri = gio::SimpleAction::new("apri_cartella", None);
    let (w, s) = (finestra.clone(), stato.clone());
    apri.connect_activate(move |_, _| {
        let s = s.clone();
        gtk::FileDialog::builder()
            .title(t(Testo::ApriCartella))
            .build()
            .select_folder(Some(&w), gio::Cancellable::NONE, move |scelta| {
                if let Ok(cartella) = scelta {
                    let mut nuove = Vec::new();
                    raccogli(&uri_di(&cartella), &mut nuove);
                    s.borrow_mut().aggiungi_e_suona(nuove);
                }
            });
    });
    finestra.add_action(&apri);

    let salva = gio::SimpleAction::new("salva_playlist", None);
    let (w, s) = (finestra.clone(), stato.clone());
    salva.connect_activate(move |_, _| {
        let s = s.clone();
        gtk::FileDialog::builder()
            .title(t(Testo::SalvaPlaylist))
            .initial_name("playlist.m3u")
            .build()
            .save(Some(&w), gio::Cancellable::NONE, move |scelta| {
                if let Ok(file) = scelta {
                    s.borrow_mut().salva_playlist(&file);
                }
            });
    });
    finestra.add_action(&salva);

    let modello = gio::Menu::new();
    let file = gio::Menu::new();
    file.append(Some(t(Testo::ApriCartella)), Some("win.apri_cartella"));
    file.append(Some(t(Testo::SalvaPlaylist)), Some("win.salva_playlist"));
    modello.append_section(None, &file);
    let brano = gio::Menu::new();
    brano.append(Some(t(Testo::Cestina)), Some("win.cestina"));
    modello.append_section(None, &brano);
    let generale = gio::Menu::new();
    generale.append(Some(t(Testo::Preferenze)), Some("win.preferenze"));
    generale.append(Some(t(Testo::Informazioni)), Some("win.informazioni"));
    modello.append_section(None, &generale);

    let menu = gtk::MenuButton::new();
    menu.set_icon_name("open-menu-symbolic");
    menu.set_tooltip_text(Some(t(Testo::Menu)));
    menu.add_css_class("flat");
    menu.set_menu_model(Some(&modello));
    menu
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

    let indietro = pulsante(
        "media-skip-backward-symbolic",
        &format!("{} (Ctrl+←)", t(Testo::Indietro)),
    );
    let pulsante_play = pulsante(
        "media-playback-start-symbolic",
        &format!("{} ({})", t(Testo::PlayPausa), t(Testo::TastoSpazio)),
    );
    let ferma = pulsante(
        "media-playback-stop-symbolic",
        &format!("{} (Ctrl+.)", t(Testo::Stop)),
    );
    let avanti = pulsante(
        "media-skip-forward-symbolic",
        &format!("{} (Ctrl+→)", t(Testo::Avanti)),
    );

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
        coda: Coda::default(),
        titolo,
        tempo,
        barra: barra.clone(),
        pulsante_play: pulsante_play.clone(),
        volume: volume.clone(),
        dettagli,
        qualita: Qualita::default(),
        impostazioni,
        elenco: elenco.clone(),
        vista: crea_vista_playlist(&elenco, &segno),
        segno: segno.clone(),
        avviso_fino: None,
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
    azioni_lettore(&finestra, &stato);

    // Niente "Ingrandisci": un mini lettore a tutto schermo non ha senso.
    // La barra del titolo mostra solo Riduci e Chiudi, qualunque sia l'impostazione
    // di GNOME; e se lo si ingrandisce lo stesso (doppio clic, Super+Su) torna com'era.
    let barra_titolo = gtk::HeaderBar::new();
    barra_titolo.set_decoration_layout(Some(":minimize,close"));
    finestra.set_titlebar(Some(&barra_titolo));
    finestra.connect_maximized_notify(|w| {
        if w.is_maximized() {
            w.unmaximize();
        }
    });

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
        let uri = if let Ok(lista) = valore.get::<gdk::FileList>() {
            lista.files().iter().map(uri_di).collect()
        } else if let Ok(testo) = valore.get::<String>() {
            voci::uri_da_righe(&testo)
        } else {
            return false;
        };
        let mut nuove = Vec::new();
        for u in &uri {
            raccogli(u, &mut nuove);
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
    // L'icona dell'app, per la finestra e per "Informazioni".
    let app = base.join("hicolor").join("scalable").join("apps");
    let file_app = app.join(format!("{APP_ID}.svg"));
    let icona_app = include_str!("../data/icons/io.github.andreae75.Rustabile.svg");
    if std::fs::read_to_string(&file_app).ok().as_deref() != Some(icona_app) {
        let _ = std::fs::create_dir_all(&app);
        let _ = std::fs::write(&file_app, icona_app);
    }
    gtk::IconTheme::for_display(display).add_search_path(&base);
    gtk::Window::set_default_icon_name(APP_ID);
}

fn main() -> glib::ExitCode {
    gst::init().expect("impossibile inizializzare GStreamer");
    lingua::inizializza(&Impostazioni::carica().lingua, &glib::language_names());

    // HANDLES_OPEN: accettiamo file da riga di comando e da "Apri con" di Nautilus.
    // GApplication garantisce una sola istanza: se Rustabile è già aperto, i file
    // aperti dopo arrivano alla finestra esistente invece di aprirne un'altra.
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();
    app.connect_startup(|app| {
        carica_stile();
        // Canc: sposta nel Cestino il brano in corso.
        app.set_accels_for_action("win.cestina", &["Delete"]);
        app.set_accels_for_action("win.apri_cartella", &["<Control>o"]);
        app.set_accels_for_action("win.salva_playlist", &["<Control>s"]);
        // Comandi del lettore: scorciatoie comuni dei lettori GNOME, tasti multimediali
        // della tastiera e, per nostalgia, i tasti dei vecchi lettori (Z X C V B).
        for (azione, tasti) in [
            (
                "win.play_pausa",
                &[
                    "space",
                    "<Control>space",
                    "AudioPlay",
                    "AudioPause",
                    "x",
                    "c",
                ][..],
            ),
            ("win.ferma", &["<Control>period", "AudioStop", "v"][..]),
            (
                "win.avanti",
                &["<Control>Right", "<Alt>Right", "AudioNext", "b"][..],
            ),
            (
                "win.indietro",
                &["<Control>Left", "<Alt>Left", "AudioPrev", "z"][..],
            ),
            ("win.volume_su", &["<Control>Up"][..]),
            ("win.volume_giu", &["<Control>Down"][..]),
        ] {
            app.set_accels_for_action(azione, tasti);
        }
    });

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
            raccogli(&uri_di(f), &mut nuove);
        }
        stato.borrow_mut().aggiungi_e_suona(nuove);
        w.present();
    });

    app.run()
}
