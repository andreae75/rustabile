# Rustabile

Un mini lettore musicale per GNOME, scritto in Rust: le funzioni di base dei lettori di una volta, niente di più.

- Trascina file, cartelle o playlist (`.m3u`, `.m3u8`, `.pls`) nella finestra per riempire la coda. Le cartelle vengono esplorate in ordine alfabetico, sottocartelle comprese.
- Radio: il pulsante con l'antenna accetta l'indirizzo di uno stream o di una playlist online, e mostra il titolo del brano in onda se la radio lo trasmette.
- Pulsante playlist: elenco dei brani in coda, un clic per farne partire uno, e "Svuota".
- Menu con Preferenze (sempre in primo piano, trascinamento che suona subito o accoda) e Informazioni. Le preferenze e il volume stanno in `~/.config/rustabile/impostazioni.ini`.
- Raccolte di radio per genere: incolla nel pulsante radio, o trascina dal browser, il link "raw" di una playlist come quelle di [m3u-radio-music-playlists](https://github.com/junguler/m3u-radio-music-playlists).
- Play / pausa, stop, avanti, indietro (se il brano è avanti più di 3 secondi ricomincia), barra di avanzamento, volume.
- GTK4 puro, quindi prende il tema di GNOME; l'audio passa da GStreamer, lo stesso motore degli altri lettori di GNOME.

Formati: quelli che GStreamer sa leggere sul sistema (MP3, FLAC, Ogg, Opus, M4A/AAC, WAV…). Le playlist online si leggono tramite GIO, quindi serve `gvfs` (presente di serie su GNOME).

## Compilare

Servono Rust e le librerie di sviluppo di GTK4 e GStreamer. Su Ubuntu:

```bash
sudo apt install libgtk-4-dev libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev
```

Poi:

```bash
cargo run --release
```

## Idee per farlo crescere

- Leggere titolo, artista e copertina dai tag (crate `lofty`) invece di usare il nome del file.
- Riordinare e togliere brani dalla playlist.
- Aprire i file passati da riga di comando o con "Apri con" di Nautilus (`ApplicationFlags::HANDLES_OPEN`).
- Comandi multimediali di GNOME e tasti della tastiera via MPRIS (crate `mpris-server`).
- Ripetizione e ordine casuale.
- File `.desktop` e icona per averlo nel menu delle applicazioni.

## Licenza

MIT: vedi [LICENSE](LICENSE). Fatene quello che volete.
