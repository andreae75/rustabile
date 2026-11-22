#!/usr/bin/env bash
# Installa Rustabile per l'utente corrente (niente sudo):
# programma in ~/.local/bin, lanciatore e icona in ~/.local/share.
# Con --predefinito lo rende anche il lettore predefinito per i file audio.
set -euo pipefail
cd "$(dirname "$0")"

ID=io.github.andreae75.Rustabile
DATI="${XDG_DATA_HOME:-$HOME/.local/share}"

cargo install --path . --root "$HOME/.local"
install -Dm644 "data/$ID.desktop" "$DATI/applications/$ID.desktop"
install -Dm644 "data/icons/$ID.svg" "$DATI/icons/hicolor/scalable/apps/$ID.svg"
update-desktop-database "$DATI/applications" 2>/dev/null || true
gtk-update-icon-cache -f -t "$DATI/icons/hicolor" 2>/dev/null || true

if [[ "${1:-}" == "--predefinito" ]]; then
    tipi=$(sed -n 's/^MimeType=//p' "data/$ID.desktop" | tr ';' ' ')
    # shellcheck disable=SC2086
    xdg-mime default "$ID.desktop" $tipi
    echo "Rustabile è ora il lettore predefinito per i file audio."
fi
echo "Rustabile installato in $HOME/.local/bin/rustabile"
