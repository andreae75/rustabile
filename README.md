# Rustabile

Un mini lettore musicale per GNOME, scritto in Rust: le funzioni di base dei lettori di una volta, niente di più.

- Trascina file, cartelle o playlist (`.m3u`, `.m3u8`, `.pls`) nella finestra per riempire la coda. Le cartelle vengono esplorate in ordine alfabetico, sottocartelle comprese.
- Radio: il pulsante con la radiolina (icona disegnata apposta, in `data/icons`) accetta l'indirizzo di uno stream o di una playlist online, e mostra il nome della stazione e il brano in onda se la radio li trasmette. Con le radio la barra di avanzamento è grigia, perché non si può andare avanti e indietro.
- Pannello playlist sotto la finestra (si apre e chiude col pulsante a lista): elenco dei brani, un clic per farne partire uno, conteggio e "Svuota". Regge anche playlist di migliaia di radio.
- Menu con Preferenze (trascinamento che suona subito o accoda, lingua) e Informazioni. Le preferenze e il volume stanno in `~/.config/rustabile/impostazioni.ini`.
- Sempre in primo piano: tasto destro sul titolo della finestra (o Alt+Spazio) e "Sempre in primo piano", la funzione di GNOME che vale per ogni app.
- Raccolte di radio per genere: incolla nel pulsante radio, o trascina dal browser, il link "raw" di una playlist come quelle di [m3u-radio-music-playlists](https://github.com/junguler/m3u-radio-music-playlists).
- **Canc** (o menu ☰ → "Sposta il brano nel Cestino"): il file del brano in corso va nel Cestino (si recupera da lì), esce dalla playlist e parte il successivo. Per le radio, le toglie solo dalla playlist.
- Play / pausa, stop, avanti, indietro (se il brano è avanti più di 3 secondi ricomincia), barra di avanzamento, volume.
- Interfaccia in italiano, inglese, tedesco, spagnolo e francese: segue la lingua del desktop, oppure si sceglie nelle Preferenze. Per provarne una al volo: `RUSTABILE_LINGUA=fr rustabile`.
- GTK4 puro, quindi prende il tema di GNOME; l'audio passa da GStreamer, lo stesso motore degli altri lettori di GNOME.

Formati: quelli che GStreamer sa leggere sul sistema (MP3, FLAC, Ogg, Opus, M4A/AAC, WAV…). Le playlist online si leggono tramite GIO, quindi serve `gvfs` (presente di serie su GNOME).

## Compilare

Servono Rust e le librerie di sviluppo di GTK4 e GStreamer. Su Ubuntu:

```bash
sudo apt install libgtk-4-dev libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev
```

Per provarlo al volo:

```bash
cargo run --release
```

Per installarlo (programma in `~/.local/bin`, lanciatore e icona nel menu delle applicazioni):

```bash
./installa.sh
```

Con `./installa.sh --predefinito` diventa anche il lettore predefinito per i file audio.

## Idee per farlo crescere

- Leggere titolo, artista e copertina dai tag (crate `lofty`) invece di usare il nome del file.
- Riordinare e togliere brani dalla playlist.
- Aprire i file passati da riga di comando o con "Apri con" di Nautilus (`ApplicationFlags::HANDLES_OPEN`).
- Comandi multimediali di GNOME e tasti della tastiera via MPRIS (crate `mpris-server`).
- Ripetizione e ordine casuale.

## Icona

L'icona (`data/icons/`) è un disegno originale: un ingranaggio color ruggine con una nota musicale. Richiama Rust senza usarne il logo, che è un marchio della Rust Foundation.

## Licenza

MIT: vedi [LICENSE](LICENSE). Fatene quello che volete.
