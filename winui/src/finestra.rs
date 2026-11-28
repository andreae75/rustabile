//! La finestra: stato del lettore, comandi e disegno.

use std::rc::Rc;
use std::time::{Duration, Instant};

use rustabile_nucleo::voci::{self, percorso, uri_da_percorso};
use rustabile_nucleo::{Coda, Impostazioni, Qualita, Testo, Voce, lingua, mm_ss, t};
use windows::Web::Http::HttpClient;
use windows::Win32::Globalization::GetUserDefaultLocaleName;
use windows::core::HSTRING;
use windows_reactor::*;

use crate::audio::{Audio, Eventi};
use crate::icona;
use crate::tasti::{self, Comando};

const CODICI_LINGUA: [&str; 6] = ["auto", "it", "en", "de", "es", "fr"];
const LARGHEZZA: f64 = 480.0;
// Le altezze delle parti, per fare la finestra grande quanto il contenuto.
const ALTEZZA_BASE: f64 = 172.0;
const ALTEZZA_RADIO: f64 = 32.0;
const ALTEZZA_VOCE: f64 = 32.0;
const ALTEZZA_PIEDE: f64 = 36.0;
/// Oltre questo numero di voci l'elenco scorre.
const VOCI_A_VISTA: usize = 8;
/// Preferenze e Informazioni stanno dentro la finestra: servono almeno questi punti.
const ALTEZZA_RIQUADRO: f64 = 460.0;

#[derive(Clone, Copy, PartialEq)]
enum Riquadro {
    Nessuno,
    Preferenze,
    Informazioni,
}

#[derive(Clone)]
pub enum Messaggio {
    Tick,
    Tasto(Comando),
    PlayPausa,
    Ferma,
    Avanti,
    Indietro,
    Barra(f64),
    Volume(f64),
    Suona(usize),
    Svuota,
    MostraPlaylist,
    MostraRadio,
    Indirizzo(String),
    AscoltaRadio,
    Trascinati(DroppedData),
    Menu(String),
    /// Voci pronte (cartelle e playlist si leggono fuori dalla finestra);
    /// il secondo valore dice se far partire subito la prima.
    Aggiungi(Vec<Voce>, bool),
    Salva(Option<std::path::PathBuf>),
    ChiudiRiquadro,
    TrascinaSuona(bool),
    Lingua(Option<usize>),
}

struct Rustabile {
    audio: Audio,
    coda: Coda,
    qualita: Qualita,
    impostazioni: Impostazioni,
    titolo: String,
    tempo: String,
    dettagli: String,
    posizione: f64,
    durata: Option<f64>,
    /// L'ultimo valore messo da noi nella barra: un valore diverso viene dall'utente.
    barra: f64,
    radio_aperta: bool,
    indirizzo: String,
    riquadro: Riquadro,
    avviso: Option<(String, Instant)>,
    /// Cambia quando cambia l'elenco, per ridisegnarlo.
    revisione: u64,
    /// Il volume cambiato si salva al giro successivo del timer.
    da_salvare: bool,
    icona_messa: bool,
}

/// Il contenuto di un file o di un indirizzo (per le playlist online).
fn leggi(uri: &str) -> Option<Vec<u8>> {
    if let Some(p) = percorso(uri) {
        return std::fs::read(p).ok();
    }
    let indirizzo = windows::Foundation::Uri::CreateUri(&HSTRING::from(uri)).ok()?;
    let risposta = HttpClient::new()
        .ok()?
        .GetAsync(&indirizzo)
        .ok()?
        .join()
        .ok()?;
    risposta.EnsureSuccessStatusCode().ok()?;
    let testo = risposta
        .Content()
        .ok()?
        .ReadAsStringAsync()
        .ok()?
        .join()
        .ok()?;
    Some(testo.to_string().into_bytes())
}

fn raccogli_tutti(uri: &[String]) -> Vec<Voce> {
    let mut nuove = Vec::new();
    for u in uri {
        voci::raccogli(u, &leggi, &mut nuove);
    }
    nuove
}

impl Rustabile {
    fn aggiungi_in_sottofondo(
        &self,
        contesto: &ComponentContext<Self>,
        uri: Vec<String>,
        suona: bool,
    ) {
        contesto.spawn_background(move |_| Messaggio::Aggiungi(raccogli_tutti(&uri), suona));
    }

    fn timer(contesto: &ComponentContext<Self>) {
        contesto.spawn_background(|_| {
            std::thread::sleep(Duration::from_millis(300));
            Messaggio::Tick
        });
    }

    fn suona(&mut self, indice: usize) {
        let Some(voce) = self.coda.voci.get(indice) else {
            return;
        };
        let uri = voce.uri.clone();
        let nome = voce.nome.clone();
        self.coda.corrente = Some(indice);
        self.titolo = self.coda.testo_riga(indice);
        self.qualita = Qualita::per(&uri);
        self.dettagli.clear();
        self.posizione = 0.0;
        self.durata = None;
        self.barra = 0.0;
        if let Err(e) = self.audio.suona(&uri, &nome) {
            self.avvisa(e.message());
        }
    }

    fn ferma(&mut self) {
        self.audio.chiudi();
        self.posizione = 0.0;
        self.barra = 0.0;
        self.durata = None;
        self.tempo = "00:00".into();
    }

    fn play_pausa(&mut self) {
        match self.coda.corrente {
            None if !self.coda.voci.is_empty() => self.suona(0),
            None => {}
            Some(i) if !self.audio.aperto() => self.suona(i),
            Some(_) if self.audio.sta_suonando() => self.audio.pausa(),
            Some(_) => self.audio.riprendi(),
        }
    }

    fn avanti(&mut self) {
        if let Some(prossima) = self.coda.successiva() {
            self.suona(prossima);
        } else if self.coda.corrente.is_some() {
            // Fine della playlist: ci fermiamo e ripartiamo dall'inizio al prossimo play.
            self.ferma();
            self.coda.corrente = None;
        }
    }

    /// Come nei vecchi lettori: se il brano è avanti più di 3 secondi ricomincia,
    /// altrimenti torna al brano precedente.
    fn indietro(&mut self) {
        let Some(i) = self.coda.corrente else {
            return;
        };
        if i == 0 || self.audio.posizione() > 3.0 {
            self.audio.vai_a(0.0);
        } else {
            self.suona(i - 1);
        }
    }

    fn cambia_volume(&mut self, v: f64) {
        let v = v.clamp(0.0, 1.0);
        if (v - self.impostazioni.volume).abs() > 0.001 {
            self.impostazioni.volume = v;
            self.audio.volume(v);
            self.da_salvare = true;
        }
    }

    fn aggiungi(&mut self, nuove: Vec<Voce>, suona: bool) {
        if nuove.is_empty() {
            return;
        }
        let prima_nuova = self.coda.voci.len();
        self.coda.voci.extend(nuove);
        self.revisione += 1;
        if suona || self.coda.corrente.is_none() {
            self.suona(prima_nuova);
        }
    }

    fn svuota(&mut self) {
        self.ferma();
        self.coda.svuota();
        self.revisione += 1;
        self.titolo = t(Testo::TrascinaQui).into();
        self.dettagli.clear();
    }

    /// Sposta nel Cestino il file del brano in corso, lo toglie dalla playlist e
    /// passa al successivo. Una radio non è un file: la togliamo solo dalla playlist.
    fn cestina(&mut self) {
        let Some(i) = self.coda.corrente else { return };
        let Voce { uri, nome, origine } = self.coda.voci[i].clone();
        self.audio.chiudi();
        let file = percorso(&uri);
        let mut da_dove = String::new();
        if let Some(p) = &file {
            // Windows può tenere il file aperto ancora per un attimo.
            let mut esito = trash::delete(p);
            for _ in 0..5 {
                if esito.is_ok() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
                esito = trash::delete(p);
            }
            if let Err(errore) = esito {
                self.suona(i);
                self.avvisa(format!("{} {errore}", t(Testo::ErroreCestino)));
                return;
            }
            if let Some(origine) = &origine {
                if voci::togli_da_playlist(origine, &uri) {
                    if let Some(lista) = origine.file_name() {
                        da_dove = format!(" — {}", lista.to_string_lossy());
                    }
                }
            }
        }
        self.coda.togli_corrente();
        self.revisione += 1;
        if i < self.coda.voci.len() {
            self.suona(i);
        } else {
            self.ferma();
            self.titolo = t(Testo::TrascinaQui).into();
        }
        self.avvisa(format!("{} {nome}{da_dove}", t(Testo::SpostatoNelCestino)));
    }

    fn avvisa(&mut self, testo: impl Into<String>) {
        let testo = testo.into();
        self.dettagli = testo.clone();
        self.avviso = Some((testo, Instant::now() + Duration::from_secs(4)));
    }

    // Le finestre di scelta file girano su un altro thread: aperte dentro il
    // ciclo della finestra la farebbero rientrare in sé stessa.
    fn apri_cartella(&self, contesto: &ComponentContext<Self>) {
        let titolo = t(Testo::ApriCartella);
        contesto.spawn_background(move |_| {
            let uri: Vec<String> = rfd::FileDialog::new()
                .set_title(titolo)
                .pick_folder()
                .iter()
                .map(|p| uri_da_percorso(p))
                .collect();
            Messaggio::Aggiungi(raccogli_tutti(&uri), true)
        });
    }

    fn salva_playlist(&self, contesto: &ComponentContext<Self>) {
        let titolo = t(Testo::SalvaPlaylist);
        contesto.spawn_background(move |_| {
            Messaggio::Salva(
                rfd::FileDialog::new()
                    .set_title(titolo)
                    .set_file_name("playlist.m3u")
                    .add_filter("M3U", &["m3u", "m3u8"])
                    .save_file(),
            )
        });
    }

    fn comando(&mut self, comando: Comando, contesto: &ComponentContext<Self>) {
        match comando {
            Comando::PlayPausa => self.play_pausa(),
            Comando::Ferma => self.ferma(),
            Comando::Avanti => self.avanti(),
            Comando::Indietro => self.indietro(),
            Comando::VolumeSu => self.cambia_volume(self.impostazioni.volume + 0.05),
            Comando::VolumeGiu => self.cambia_volume(self.impostazioni.volume - 0.05),
            Comando::Cestina => self.cestina(),
            Comando::ApriCartella => self.apri_cartella(contesto),
            Comando::SalvaPlaylist => self.salva_playlist(contesto),
        }
    }

    fn voce_di_menu(&mut self, voce: &str, contesto: &ComponentContext<Self>) {
        if voce == t(Testo::ApriCartella) {
            self.apri_cartella(contesto);
        } else if voce == t(Testo::SalvaPlaylist) {
            self.salva_playlist(contesto);
        } else if voce == t(Testo::Cestina) {
            self.cestina();
        } else if voce == t(Testo::Preferenze) {
            self.riquadro = Riquadro::Preferenze;
        } else if voce == t(Testo::Informazioni) {
            self.riquadro = Riquadro::Informazioni;
        }
    }

    /// Ogni 300 ms: avvisi dal motore audio, tempo, barra e dettagli.
    fn tick(&mut self) {
        // Al primo giro la finestra esiste di sicuro.
        if !self.icona_messa {
            self.icona_messa = true;
            icona::applica();
        }
        let eventi = self.audio.eventi.clone();
        if Eventi::preso(&eventi.avanti) {
            self.avanti();
        }
        if Eventi::preso(&eventi.indietro) {
            self.indietro();
        }
        if Eventi::preso(&eventi.finito) {
            self.avanti();
        } else if Eventi::preso(&eventi.errore) {
            eprintln!("Errore di riproduzione: {}", self.titolo);
            self.avanti();
        }
        if self.da_salvare {
            self.da_salvare = false;
            self.impostazioni.salva();
        }
        if !self.audio.aperto() {
            return;
        }

        self.posizione = self.audio.posizione();
        self.durata = self.audio.durata().filter(|_| !self.qualita.radio);
        let ascoltati = mm_ss(self.posizione as u64);
        self.tempo = match self.durata {
            Some(d) => format!("{ascoltati} / {}", mm_ss(d as u64)),
            None => format!("{ascoltati}  ● LIVE"),
        };
        self.barra = self.durata.map_or(0.0, |d| self.posizione.min(d));

        if let Some((testo, fino)) = &self.avviso {
            if Instant::now() < *fino {
                self.dettagli.clone_from(testo);
                return;
            }
            self.avviso = None;
        }
        let audio = self.audio.decodificato();
        // L'estensione dice il formato meglio del sottotipo di Windows ("PCM", "Float").
        if self.qualita.estensione.is_empty() {
            self.qualita.codec = audio.codec;
        }
        // Per i file senza perdita il bitrate si stima da dimensione e durata.
        if !self.qualita.senza_perdita() {
            self.qualita.bitrate = audio.bitrate;
        }
        self.dettagli = self.qualita.dettagli(
            audio.bit,
            audio.frequenza,
            audio.canali,
            self.durata.map(|d| d.round() as u64),
        );
    }

    /// L'altezza che serve a quello che si vede adesso.
    fn altezza(&self) -> f64 {
        let mut altezza = ALTEZZA_BASE;
        if self.radio_aperta {
            altezza += ALTEZZA_RADIO;
        }
        if self.impostazioni.mostra_playlist {
            let voci = self.coda.voci.len().min(VOCI_A_VISTA);
            altezza += voci as f64 * ALTEZZA_VOCE + ALTEZZA_PIEDE;
        }
        if self.riquadro != Riquadro::Nessuno {
            altezza = altezza.max(ALTEZZA_RIQUADRO);
        }
        altezza
    }

    fn pulsante(icona: Symbol, descrizione: String, al_clic: Callback<()>) -> View {
        Button::new()
            .style(ButtonStyle::Subtle)
            .on_click(al_clic)
            .automation_name(descrizione.clone())
            .content(SymbolIcon::new().symbol(icona))
            .tooltip(descrizione)
    }

    fn comandi(&self, vista: &mut ViewContext<Self>) -> View {
        let play = if self.audio.sta_suonando() {
            Symbol::Pause
        } else {
            Symbol::Play
        };
        let menu = Menu::new(
            [
                MenuItem::item("apri", t(Testo::ApriCartella)),
                MenuItem::item("salva", t(Testo::SalvaPlaylist)),
                MenuItem::separator("s1"),
                MenuItem::item("cestina", t(Testo::Cestina)),
                MenuItem::separator("s2"),
                MenuItem::item("preferenze", t(Testo::Preferenze)),
                MenuItem::item("informazioni", t(Testo::Informazioni)),
            ],
            vista.callback(Messaggio::Menu),
        );
        StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(2.0)
            .children((
                Self::pulsante(
                    Symbol::Previous,
                    format!("{} (Ctrl+←)", t(Testo::Indietro)),
                    vista.message(Messaggio::Indietro),
                ),
                Self::pulsante(
                    play,
                    format!("{} ({})", t(Testo::PlayPausa), t(Testo::TastoSpazio)),
                    vista.message(Messaggio::PlayPausa),
                ),
                Self::pulsante(
                    Symbol::Stop,
                    format!("{} (Ctrl+.)", t(Testo::Stop)),
                    vista.message(Messaggio::Ferma),
                ),
                Self::pulsante(
                    Symbol::Next,
                    format!("{} (Ctrl+→)", t(Testo::Avanti)),
                    vista.message(Messaggio::Avanti),
                ),
                Self::pulsante(
                    Symbol::List,
                    t(Testo::MostraPlaylist).into(),
                    vista.message(Messaggio::MostraPlaylist),
                ),
                Self::pulsante(
                    Symbol::World,
                    t(Testo::AggiungiRadio).into(),
                    vista.message(Messaggio::MostraRadio),
                ),
                SymbolIcon::new().symbol(Symbol::Volume),
                Slider::new()
                    .minimum(0.0)
                    .maximum(100.0)
                    .step_frequency(1.0)
                    .value((self.impostazioni.volume * 100.0).round())
                    .on_value_changed(vista.callback(|v: f64| Messaggio::Volume(v / 100.0)))
                    .width(110.0)
                    .vertical_alignment(VerticalAlignment::Center),
                // Niente suggerimento qui: insieme al menu fa chiudere WinUI.
                Button::new()
                    .style(ButtonStyle::Subtle)
                    .automation_name(t(Testo::Menu))
                    .content(SymbolIcon::new().symbol(Symbol::GlobalNavigationButton))
                    .menu(menu),
            ))
    }

    fn riga_radio(&self, vista: &mut ViewContext<Self>) -> View {
        if !self.radio_aperta {
            return View::empty();
        }
        StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(6.0)
            .children((
                TextBox::new()
                    .text(self.indirizzo.clone())
                    .placeholder_text(t(Testo::IndirizzoRadio))
                    .on_text_changed(vista.callback(Messaggio::Indirizzo))
                    .width(320.0),
                Button::new()
                    .style(ButtonStyle::Accent)
                    .on_click(vista.message(Messaggio::AscoltaRadio))
                    .content(t(Testo::Ascolta)),
            ))
    }

    fn pannello(&self, vista: &mut ViewContext<Self>) -> View {
        if !self.impostazioni.mostra_playlist {
            return View::empty();
        }
        // Solo le righe a vista vengono create: regge migliaia di radio.
        let righe: Rc<Vec<String>> = Rc::new(
            (0..self.coda.voci.len())
                .map(|i| self.coda.testo_riga(i))
                .collect(),
        );
        let corrente = self.coda.corrente;
        let suona = vista.callback(Messaggio::Suona);
        let sorgente = VirtualSource::new(
            self.revisione,
            righe.len(),
            |i| i as u64,
            move |i| {
                let suona = suona.clone();
                let segno = if corrente == Some(i) { "▶ " } else { "    " };
                Button::new()
                    .style(ButtonStyle::Subtle)
                    .horizontal_content_alignment(HorizontalAlignment::Left)
                    .on_click(move || {
                        let _ = suona.call(i);
                    })
                    .width(LARGHEZZA - 40.0)
                    .content(
                        TextBlock::new()
                            .text(format!("{segno}{}", righe[i]))
                            .text_trimming(TextTrimming::CharacterEllipsis),
                    )
            },
        );
        let n = self.coda.voci.len();
        Grid::new()
            .rows([GridLength::STAR, GridLength::Auto])
            .grid_row(4)
            .children((
                ScrollViewer::new().content(ItemsRepeater::new().virtual_source(sorgente)),
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(12.0)
                    .grid_row(1)
                    .children((
                        TextBlock::new()
                            .text(lingua::brani(n))
                            .vertical_alignment(VerticalAlignment::Center),
                        Button::new()
                            .style(ButtonStyle::Subtle)
                            .on_click(vista.message(Messaggio::Svuota))
                            .content(t(Testo::Svuota)),
                    )),
            ))
    }

    fn preferenze(&self, vista: &mut ViewContext<Self>) -> View {
        let nomi = [
            t(Testo::LinguaAutomatica),
            "Italiano",
            "English",
            "Deutsch",
            "Español",
            "Français",
        ];
        let scelta = CODICI_LINGUA
            .iter()
            .position(|c| *c == self.impostazioni.lingua)
            .unwrap_or(0);
        let testo = |s: &str| {
            TextBlock::new()
                .text(s)
                .text_wrapping(TextWrapping::Wrap)
                .max_width(320.0)
        };
        ContentDialog::new()
            .is_open(self.riquadro == Riquadro::Preferenze)
            .title(t(Testo::Preferenze))
            .close_button_text(t(Testo::Chiudi))
            .on_closed(vista.callback(|_| Messaggio::ChiudiRiquadro))
            .content(
                StackPanel::new().spacing(8.0).children((
                    testo(t(Testo::TrascinaSuona)),
                    testo(t(Testo::TrascinaSuonaSpiegazione)).font_size(12.0),
                    ToggleSwitch::new()
                        .is_on(self.impostazioni.trascina_suona)
                        .on_toggled(vista.callback(Messaggio::TrascinaSuona)),
                    testo(t(Testo::TitoloLingua)),
                    testo(t(Testo::LinguaSpiegazione)).font_size(12.0),
                    ComboBox::new()
                        .items_source(nomi)
                        .selected_index(scelta)
                        .on_selection_changed(vista.callback(Messaggio::Lingua)),
                )),
            )
    }

    fn informazioni(&self, vista: &mut ViewContext<Self>) -> View {
        ContentDialog::new()
            .is_open(self.riquadro == Riquadro::Informazioni)
            .title(concat!("Rustabile ", env!("CARGO_PKG_VERSION")))
            .close_button_text(t(Testo::Chiudi))
            .on_closed(vista.callback(|_| Messaggio::ChiudiRiquadro))
            .content(
                TextBlock::new()
                    .text(format!(
                        "{}\n\nandreae75 · MIT\nhttps://github.com/andreae75/rustabile",
                        t(Testo::DescrizioneWindows)
                    ))
                    .text_wrapping(TextWrapping::Wrap)
                    .is_text_selection_enabled(true)
                    .max_width(320.0),
            )
    }
}

impl Component for Rustabile {
    type Input = ();
    type Message = Messaggio;

    fn create(_: &(), contesto: &ComponentContext<Self>) -> Self {
        let impostazioni = Impostazioni::carica();
        let audio = Audio::nuovo().expect("l'audio di Windows non è disponibile");
        audio.volume(impostazioni.volume);

        let tastiera = contesto.sender();
        tasti::aggancia(move |c| {
            tastiera.send(Messaggio::Tasto(c));
        });
        Self::timer(contesto);

        let rustabile = Rustabile {
            audio,
            coda: Coda::default(),
            qualita: Qualita::default(),
            impostazioni,
            titolo: t(Testo::TrascinaQui).into(),
            tempo: "00:00".into(),
            dettagli: String::new(),
            posizione: 0.0,
            durata: None,
            barra: 0.0,
            radio_aperta: false,
            indirizzo: String::new(),
            riquadro: Riquadro::Nessuno,
            avviso: None,
            revisione: 0,
            da_salvare: false,
            icona_messa: false,
        };
        // File passati da riga di comando o con "Apri con".
        let da_fuori: Vec<String> = std::env::args_os()
            .skip(1)
            .map(|a| voci::uri_da_testo(&a.to_string_lossy()))
            .collect();
        if !da_fuori.is_empty() {
            rustabile.aggiungi_in_sottofondo(contesto, da_fuori, true);
        }
        rustabile
    }

    fn update(&mut self, messaggio: Messaggio, contesto: &ComponentContext<Self>) {
        match messaggio {
            Messaggio::Tick => {
                self.tick();
                Self::timer(contesto);
            }
            Messaggio::Tasto(c) => self.comando(c, contesto),
            Messaggio::PlayPausa => self.play_pausa(),
            Messaggio::Ferma => self.ferma(),
            Messaggio::Avanti => self.avanti(),
            Messaggio::Indietro => self.indietro(),
            Messaggio::Barra(v) => {
                // La barra avvisa anche quando la muoviamo noi: quello va ignorato.
                if self.durata.is_some() && (v - self.barra).abs() > 1.0 {
                    self.barra = v;
                    self.audio.vai_a(v);
                }
            }
            Messaggio::Volume(v) => self.cambia_volume(v),
            Messaggio::Suona(i) => self.suona(i),
            Messaggio::Svuota => self.svuota(),
            Messaggio::MostraPlaylist => {
                self.impostazioni.mostra_playlist = !self.impostazioni.mostra_playlist;
                self.impostazioni.salva();
            }
            Messaggio::MostraRadio => self.radio_aperta = !self.radio_aperta,
            Messaggio::Indirizzo(testo) => self.indirizzo = testo,
            Messaggio::AscoltaRadio => {
                let testo = self.indirizzo.trim().to_string();
                if !testo.is_empty() {
                    // Una radio aggiunta a mano parte subito.
                    self.aggiungi_in_sottofondo(contesto, vec![voci::uri_da_testo(&testo)], true);
                    self.indirizzo.clear();
                    self.radio_aperta = false;
                }
            }
            Messaggio::Trascinati(dati) => {
                let uri = match dati {
                    DroppedData::StorageItems(elementi) => elementi
                        .iter()
                        .map(|e| uri_da_percorso(std::path::Path::new(&e.path)))
                        .collect(),
                    DroppedData::Text(testo) => voci::uri_da_righe(&testo),
                    DroppedData::Unsupported => Vec::new(),
                };
                let suona = self.impostazioni.trascina_suona;
                self.aggiungi_in_sottofondo(contesto, uri, suona);
            }
            Messaggio::Menu(voce) => self.voce_di_menu(&voce, contesto),
            Messaggio::Aggiungi(nuove, suona) => self.aggiungi(nuove, suona),
            Messaggio::Salva(None) => {}
            Messaggio::Salva(Some(file)) => match std::fs::write(&file, self.coda.m3u()) {
                Ok(()) => {
                    let nome = file.file_name().unwrap_or_default().to_string_lossy();
                    self.avvisa(format!("{} {nome}", t(Testo::PlaylistSalvata)));
                }
                Err(e) => self.avvisa(format!("{} {e}", t(Testo::ErroreSalvataggio))),
            },
            Messaggio::ChiudiRiquadro => self.riquadro = Riquadro::Nessuno,
            Messaggio::TrascinaSuona(si) => {
                self.impostazioni.trascina_suona = si;
                self.impostazioni.salva();
            }
            Messaggio::Lingua(indice) => {
                if let Some(codice) = indice.and_then(|i| CODICI_LINGUA.get(i)) {
                    self.impostazioni.lingua = codice.to_string();
                    self.impostazioni.salva();
                }
            }
        }
        tasti::sospendi(self.radio_aperta || self.riquadro != Riquadro::Nessuno);
    }

    fn view(&self, _: &(), vista: &mut ViewContext<Self>) -> View {
        vista.window_title("Rustabile");
        vista.window_visuals(WindowVisuals::new().client_size(LARGHEZZA, self.altezza()));

        let riga = |testo: &str| {
            TextBlock::new()
                .text(testo)
                .text_trimming(TextTrimming::CharacterEllipsis)
                .max_lines(1)
        };
        let display = StackPanel::new().spacing(2.0).children((
            riga(&self.tempo)
                .font_size(24.0)
                .font_weight(FontWeight::BOLD),
            riga(&self.titolo),
            riga(&self.dettagli).font_size(12.0),
        ));
        // Con una radio non si può andare avanti e indietro: barra grigia.
        let barra = Slider::new()
            .minimum(0.0)
            .maximum(self.durata.unwrap_or(1.0).max(1.0))
            .step_frequency(0.1)
            .value(self.barra)
            .is_enabled(self.durata.is_some())
            .on_value_changed(vista.callback(Messaggio::Barra))
            .grid_row(1);

        let trascina = DragDropPolicy::new()
            .storage_items(DragDropAction::new(DragDropOperation::Copy))
            .text(DragDropAction::new(DragDropOperation::Copy));
        Border::new()
            .background(ThemeBrush::SolidBackground)
            .padding(Thickness::uniform(10.0))
            .drop_policy(trascina)
            .on_drop(vista.callback(Messaggio::Trascinati))
            .content(
                Grid::new()
                    .rows([
                        GridLength::Auto,
                        GridLength::Auto,
                        GridLength::Auto,
                        GridLength::Auto,
                        GridLength::STAR,
                    ])
                    .row_spacing(4.0)
                    .children((
                        display,
                        barra,
                        StackPanel::new()
                            .grid_row(2)
                            .children((self.comandi(vista),)),
                        StackPanel::new()
                            .grid_row(3)
                            .children((self.riga_radio(vista),)),
                        self.pannello(vista),
                        self.preferenze(vista),
                        self.informazioni(vista),
                    )),
            )
    }
}

fn lingua_di_sistema() -> String {
    let mut nome = [0u16; 85];
    let n = unsafe { GetUserDefaultLocaleName(&mut nome) };
    String::from_utf16_lossy(&nome[..(n.max(1) - 1) as usize])
}

pub fn avvia() {
    lingua::inizializza(&Impostazioni::carica().lingua, &[lingua_di_sistema()]);
    if let Err(e) = App::run_component::<Rustabile>(()) {
        eprintln!("Impossibile aprire la finestra: {e}");
    }
}
