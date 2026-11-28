//! Quello che sappiamo del brano in corso e come lo scriviamo a schermo.

use crate::lingua::{self, Testo, t};

/// Arriva a pezzi, dai tag e dal decoder.
#[derive(Default)]
pub struct Qualita {
    /// Nome del codec dai tag ("Free Lossless Audio Codec (FLAC)").
    pub codec: Option<String>,
    /// Bitrate in bit/s dai tag (MP3 e radio di solito lo dichiarano).
    pub bitrate: Option<u32>,
    /// Dimensione del file locale, per stimare il bitrate se i tag non lo dicono.
    pub dimensione: Option<u64>,
    /// Estensione, come ripiego per il nome del formato.
    pub estensione: String,
    /// Vero per gli stream (radio): cambia come mostriamo titolo e stazione.
    pub radio: bool,
    /// Nome della stazione, se la radio lo trasmette (tag "organization", da icy-name).
    pub stazione: Option<String>,
    /// Brano in onda, se la radio lo trasmette (tag "title", da StreamTitle).
    pub in_onda: Option<String>,
}

impl Qualita {
    /// Quello che si sa già prima di aprire il brano.
    pub fn per(uri: &str) -> Self {
        let file = crate::voci::percorso(uri);
        Qualita {
            radio: file.is_none(),
            dimensione: file
                .and_then(|p| std::fs::metadata(p).ok())
                .map(|m| m.len()),
            estensione: crate::voci::estensione(uri),
            ..Default::default()
        }
    }

    /// "Free Lossless Audio Codec (FLAC)" → "FLAC"; altrimenti il nome così com'è.
    pub fn formato(&self) -> String {
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

    pub fn senza_perdita(&self) -> bool {
        let f = self.formato().to_uppercase();
        ["FLAC", "WAV", "PCM", "ALAC", "APE", "WAVPACK"]
            .iter()
            .any(|x| f.contains(x))
    }

    /// "Stazione — Brano", oppure quello che c'è dei due.
    pub fn riga_radio(&self) -> Option<String> {
        match (&self.stazione, &self.in_onda) {
            (Some(s), Some(t)) => Some(format!("{s} — {t}")),
            (Some(x), None) | (None, Some(x)) => Some(x.clone()),
            (None, None) => None,
        }
    }

    /// La riga dei dettagli: formato, bit, frequenza, canali, bitrate.
    /// `bit`, `frequenza` e `canali` vengono dall'audio decodificato, se noti.
    pub fn dettagli(
        &self,
        bit: Option<u32>,
        frequenza: Option<i32>,
        n_canali: Option<i32>,
        durata_secondi: Option<u64>,
    ) -> String {
        let mut parti = vec![self.formato()];
        if self.senza_perdita() {
            if let Some(b) = bit {
                parti.push(format!("{b} bit"));
            }
        }
        if let Some(f) = frequenza {
            parti.push(khz(f));
        }
        if let Some(n) = n_canali {
            parti.push(canali(n));
        }
        // Bitrate: dai tag se c'è, altrimenti stimato da dimensione e durata del file.
        let kbps = self.bitrate.map(|b| b / 1000).or_else(|| {
            let (dim, dur) = (self.dimensione?, durata_secondi?);
            (dur > 0).then(|| (dim * 8 / dur / 1000) as u32)
        });
        if let Some(k) = kbps {
            parti.push(format!("{k} kbps"));
        }
        parti.retain(|p| !p.is_empty());
        parti.join(" · ")
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

pub fn mm_ss(secondi: u64) -> String {
    format!("{:02}:{:02}", secondi / 60, secondi % 60)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn frequenze_e_nomi() {
        assert_eq!(khz(44100), "44,1 kHz");
        assert_eq!(khz(96000), "96 kHz");
        let q = Qualita {
            codec: Some("Free Lossless Audio Codec (FLAC)".into()),
            ..Default::default()
        };
        assert_eq!(q.formato(), "FLAC");
        assert!(q.senza_perdita());
    }
}
