//! Il trascinamento di file, cartelle, playlist e link nella finestra.
//!
//! Non usiamo quello di WinUI: non funziona nelle applicazioni avviate come
//! amministratore (microsoft-ui-xaml #7690), cioè sempre quando il Controllo
//! account utente è spento. Ci registriamo noi come destinazione OLE.

use std::cell::{Cell, RefCell};

use rustabile_nucleo::voci::{uri_da_percorso, uri_da_righe};
use windows::Win32::Foundation::{HWND, LPARAM, POINTL};
use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, TYMED_HGLOBAL};
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{
    CF_HDROP, CF_UNICODETEXT, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE, IDropTarget,
    IDropTarget_Impl, OleInitialize, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{EnumChildWindows, EnumThreadWindows};
use windows::core::{BOOL, Ref, Result};
use windows_core::implement;

type Destinatario = Box<dyn Fn(Vec<String>)>;

thread_local! {
    static DESTINATARIO: RefCell<Option<Destinatario>> = const { RefCell::new(None) };
}

fn formato(tipo: u16) -> FORMATETC {
    FORMATETC {
        cfFormat: tipo,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

fn contiene(dati: &IDataObject, tipo: u16) -> bool {
    unsafe { dati.QueryGetData(&formato(tipo)) }.is_ok()
}

/// I file trascinati (da Esplora file) o, se non ce ne sono, i link nel testo.
fn indirizzi(dati: &IDataObject) -> Vec<String> {
    unsafe {
        if let Ok(mut mezzo) = dati.GetData(&formato(CF_HDROP.0)) {
            let elenco = HDROP(mezzo.u.hGlobal.0);
            let quanti = DragQueryFileW(elenco, u32::MAX, None);
            let uri = (0..quanti)
                .map(|i| {
                    let mut nome = vec![0u16; DragQueryFileW(elenco, i, None) as usize + 1];
                    let n = DragQueryFileW(elenco, i, Some(&mut nome)) as usize;
                    uri_da_percorso(std::path::Path::new(&String::from_utf16_lossy(&nome[..n])))
                })
                .collect();
            ReleaseStgMedium(&mut mezzo);
            return uri;
        }
        let Ok(mut mezzo) = dati.GetData(&formato(CF_UNICODETEXT.0)) else {
            return Vec::new();
        };
        let blocco = mezzo.u.hGlobal;
        let inizio = GlobalLock(blocco) as *const u16;
        let mut uri = Vec::new();
        if !inizio.is_null() {
            let testo = windows::core::PCWSTR(inizio)
                .to_string()
                .unwrap_or_default();
            uri = uri_da_righe(&testo);
            let _ = GlobalUnlock(blocco);
        }
        ReleaseStgMedium(&mut mezzo);
        uri
    }
}

#[implement(IDropTarget)]
#[derive(Default)]
struct Destinazione {
    /// Se quello che si sta trascinando è roba che sappiamo aprire.
    buono: Cell<bool>,
}

impl Destinazione {
    fn rispondi(&self, effetto: *mut DROPEFFECT) {
        if let Some(effetto) = unsafe { effetto.as_mut() } {
            *effetto = if self.buono.get() {
                DROPEFFECT_COPY
            } else {
                DROPEFFECT_NONE
            };
        }
    }
}

impl IDropTarget_Impl for Destinazione_Impl {
    fn DragEnter(
        &self,
        dati: Ref<IDataObject>,
        _: MODIFIERKEYS_FLAGS,
        _: &POINTL,
        effetto: *mut DROPEFFECT,
    ) -> Result<()> {
        self.buono.set(
            dati.as_ref()
                .is_some_and(|d| contiene(d, CF_HDROP.0) || contiene(d, CF_UNICODETEXT.0)),
        );
        self.rispondi(effetto);
        Ok(())
    }

    fn DragOver(&self, _: MODIFIERKEYS_FLAGS, _: &POINTL, effetto: *mut DROPEFFECT) -> Result<()> {
        self.rispondi(effetto);
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        Ok(())
    }

    fn Drop(
        &self,
        dati: Ref<IDataObject>,
        _: MODIFIERKEYS_FLAGS,
        _: &POINTL,
        effetto: *mut DROPEFFECT,
    ) -> Result<()> {
        self.rispondi(effetto);
        let uri = dati.as_ref().map(indirizzi).unwrap_or_default();
        if !uri.is_empty() {
            DESTINATARIO.with_borrow(|d| {
                if let Some(d) = d {
                    d(uri);
                }
            });
        }
        Ok(())
    }
}

/// Da chiamare sul thread della finestra, quando la finestra esiste.
pub fn aggancia(destinatario: impl Fn(Vec<String>) + 'static) {
    DESTINATARIO.set(Some(Box::new(destinatario)));
    let destinazione: IDropTarget = Destinazione::default().into();
    unsafe {
        // Già fatto da WinUI nella forma che serve a OLE, o da fare: va bene in entrambi i casi.
        let _ = OleInitialize(None);
        let _ = EnumThreadWindows(
            GetCurrentThreadId(),
            Some(finestra_principale),
            LPARAM(&destinazione as *const IDropTarget as isize),
        );
    }
}

/// La finestra e tutte quelle che contiene: il contenuto WinUI è una di queste.
unsafe extern "system" fn finestra_principale(finestra: HWND, destinazione: LPARAM) -> BOOL {
    unsafe {
        let _ = registra(finestra, destinazione);
        let _ = EnumChildWindows(Some(finestra), Some(registra), destinazione);
    }
    true.into()
}

unsafe extern "system" fn registra(finestra: HWND, destinazione: LPARAM) -> BOOL {
    unsafe {
        let destinazione = &*(destinazione.0 as *const IDropTarget);
        let _ = RevokeDragDrop(finestra);
        if let Err(e) = RegisterDragDrop(finestra, destinazione) {
            eprintln!("Trascinamento non disponibile: {e}");
        }
    }
    true.into()
}
