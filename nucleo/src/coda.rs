//! La coda di riproduzione: le voci e quale sta suonando.

use crate::Voce;

#[derive(Default)]
pub struct Coda {
    pub voci: Vec<Voce>,
    pub corrente: Option<usize>,
}

impl Coda {
    /// "3. Titolo", come compare nel display e nel pannello.
    pub fn testo_riga(&self, i: usize) -> String {
        format!("{}. {}", i + 1, self.voci[i].nome)
    }

    /// La voce dopo quella in corso; `None` alla fine della playlist.
    pub fn successiva(&self) -> Option<usize> {
        self.corrente
            .map(|i| i + 1)
            .filter(|i| *i < self.voci.len())
    }

    /// Toglie la voce in corso. Restituisce il suo indice, dove ora c'è la successiva.
    pub fn togli_corrente(&mut self) -> Option<usize> {
        let i = self.corrente.take()?;
        self.voci.remove(i);
        Some(i)
    }

    pub fn svuota(&mut self) {
        self.voci.clear();
        self.corrente = None;
    }

    /// La playlist in formato M3U: percorsi completi per i file, indirizzi per le radio.
    pub fn m3u(&self) -> String {
        let mut testo = String::from("#EXTM3U\n");
        for voce in &self.voci {
            let dove = crate::voci::percorso(&voce.uri)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| voce.uri.clone());
            testo.push_str(&format!("#EXTINF:-1,{}\n{dove}\n", voce.nome));
        }
        testo
    }
}
