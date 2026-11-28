//! Il motore audio: il `MediaPlayer` di Windows.
//!
//! Gli avvisi di Windows (fine brano, errore, tasti multimediali) arrivano su
//! altri thread: li annotiamo in `Eventi` e la finestra li legge col suo timer.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Foundation::{TimeSpan, TypedEventHandler, Uri};
use windows::Media::Core::MediaSource;
use windows::Media::MediaPlaybackType;
use windows::Media::Playback::{
    MediaCommandEnablingRule, MediaPlaybackItem, MediaPlaybackState, MediaPlayer,
};
use windows::core::{HSTRING, Result};

#[derive(Default)]
pub struct Eventi {
    pub finito: AtomicBool,
    pub errore: AtomicBool,
    pub avanti: AtomicBool,
    pub indietro: AtomicBool,
}

impl Eventi {
    /// Vero una volta sola per ogni avviso.
    pub fn preso(bandiera: &AtomicBool) -> bool {
        bandiera.swap(false, Ordering::SeqCst)
    }
}

/// Quello che il decoder sa dell'audio del brano.
#[derive(Default)]
pub struct Decodificato {
    pub codec: Option<String>,
    pub bitrate: Option<u32>,
    pub bit: Option<u32>,
    pub frequenza: Option<i32>,
    pub canali: Option<i32>,
}

pub struct Audio {
    lettore: MediaPlayer,
    brano: Option<MediaPlaybackItem>,
    pub eventi: Arc<Eventi>,
}

impl Audio {
    pub fn nuovo() -> Result<Self> {
        let lettore = MediaPlayer::new()?;
        let eventi = Arc::new(Eventi::default());

        let e = eventi.clone();
        lettore.MediaEnded(&TypedEventHandler::new(move |_, _| {
            e.finito.store(true, Ordering::SeqCst);
            Ok(())
        }))?;
        let e = eventi.clone();
        lettore.MediaFailed(&TypedEventHandler::new(move |_, _| {
            e.errore.store(true, Ordering::SeqCst);
            Ok(())
        }))?;

        // I tasti multimediali "avanti" e "indietro", anche a finestra non attiva.
        let comandi = lettore.CommandManager()?;
        comandi
            .NextBehavior()?
            .SetEnablingRule(MediaCommandEnablingRule::Always)?;
        comandi
            .PreviousBehavior()?
            .SetEnablingRule(MediaCommandEnablingRule::Always)?;
        let e = eventi.clone();
        comandi.NextReceived(&TypedEventHandler::new(move |_, _| {
            e.avanti.store(true, Ordering::SeqCst);
            Ok(())
        }))?;
        let e = eventi.clone();
        comandi.PreviousReceived(&TypedEventHandler::new(move |_, _| {
            e.indietro.store(true, Ordering::SeqCst);
            Ok(())
        }))?;

        Ok(Audio {
            lettore,
            brano: None,
            eventi,
        })
    }

    /// Apre un indirizzo e lo fa partire; `titolo` compare nei controlli di Windows.
    pub fn suona(&mut self, uri: &str, titolo: &str) -> Result<()> {
        self.chiudi();
        let sorgente = MediaSource::CreateFromUri(&Uri::CreateUri(&HSTRING::from(uri))?)?;
        let brano = MediaPlaybackItem::Create(&sorgente)?;
        let scheda = brano.GetDisplayProperties()?;
        scheda.SetType(MediaPlaybackType::Music)?;
        scheda.MusicProperties()?.SetTitle(&HSTRING::from(titolo))?;
        brano.ApplyDisplayProperties(&scheda)?;
        self.lettore.SetSource(&brano)?;
        self.brano = Some(brano);
        self.lettore.Play()
    }

    /// Lascia il file (serve prima di spostarlo nel Cestino).
    pub fn chiudi(&mut self) {
        if self.brano.take().is_some() {
            let _ = self.lettore.Pause();
            let _ = self.lettore.SetSource(None);
        }
        self.eventi.finito.store(false, Ordering::SeqCst);
        self.eventi.errore.store(false, Ordering::SeqCst);
    }

    pub fn aperto(&self) -> bool {
        self.brano.is_some()
    }

    pub fn sta_suonando(&self) -> bool {
        self.lettore
            .PlaybackSession()
            .and_then(|s| s.PlaybackState())
            .is_ok_and(|s| {
                s == MediaPlaybackState::Playing
                    || s == MediaPlaybackState::Opening
                    || s == MediaPlaybackState::Buffering
            })
    }

    pub fn riprendi(&self) {
        let _ = self.lettore.Play();
    }

    pub fn pausa(&self) {
        let _ = self.lettore.Pause();
    }

    pub fn volume(&self, v: f64) {
        let _ = self.lettore.SetVolume(v.clamp(0.0, 1.0));
    }

    pub fn posizione(&self) -> f64 {
        self.lettore
            .PlaybackSession()
            .and_then(|s| s.Position())
            .map_or(0.0, |t| secondi(t))
    }

    /// `None` se non si sa (o non esiste, come per le radio).
    pub fn durata(&self) -> Option<f64> {
        let t = self
            .lettore
            .PlaybackSession()
            .and_then(|s| s.NaturalDuration())
            .ok()?;
        // Per le dirette Windows dichiara una durata enorme.
        Some(secondi(t)).filter(|s| *s > 0.0 && *s < 30.0 * 86400.0)
    }

    pub fn vai_a(&self, secondi: f64) {
        if let Ok(sessione) = self.lettore.PlaybackSession() {
            let _ = sessione.SetPosition(TimeSpan {
                Duration: (secondi.max(0.0) * 1e7) as i64,
            });
        }
    }

    pub fn decodificato(&self) -> Decodificato {
        let leggi = || -> Result<Decodificato> {
            let Some(brano) = &self.brano else {
                return Ok(Decodificato::default());
            };
            let p = brano.AudioTracks()?.GetAt(0)?.GetEncodingProperties()?;
            let positivo = |n: u32| Some(n).filter(|n| *n > 0);
            Ok(Decodificato {
                codec: p
                    .Subtype()
                    .ok()
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty() && !s.starts_with('{')),
                bitrate: p.Bitrate().ok().and_then(positivo),
                bit: p.BitsPerSample().ok().and_then(positivo),
                frequenza: p.SampleRate().ok().and_then(positivo).map(|n| n as i32),
                canali: p.ChannelCount().ok().and_then(positivo).map(|n| n as i32),
            })
        };
        leggi().unwrap_or_default()
    }
}

fn secondi(t: TimeSpan) -> f64 {
    t.Duration as f64 / 1e7
}
