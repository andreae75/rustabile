//! Le scorciatoie da tastiera.
//!
//! `windows-reactor` conosce pochi tasti per le sue scorciatoie (non Spazio, Canc
//! o le frecce): le leggiamo con un aggancio Windows sul solo thread della
//! finestra, quindi valgono quando Rustabile è attivo.

use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VIRTUAL_KEY, VK_CONTROL, VK_DELETE, VK_DOWN, VK_LEFT, VK_MEDIA_STOP, VK_MENU,
    VK_OEM_PERIOD, VK_RIGHT, VK_SPACE, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, HC_ACTION, SetWindowsHookExW, WH_KEYBOARD,
};

#[derive(Clone, Copy, Debug)]
pub enum Comando {
    PlayPausa,
    Ferma,
    Avanti,
    Indietro,
    VolumeSu,
    VolumeGiu,
    Cestina,
    ApriCartella,
    SalvaPlaylist,
}

type Destinatario = Box<dyn Fn(Comando)>;

thread_local! {
    static DESTINATARIO: RefCell<Option<Destinatario>> = const { RefCell::new(None) };
    static SOSPESE: Cell<bool> = const { Cell::new(false) };
}

/// Mentre si scrive in un campo di testo (o c'è una finestrella aperta)
/// i tasti restano a lui.
pub fn sospendi(si: bool) {
    SOSPESE.set(si);
}

/// Da chiamare sul thread della finestra.
pub fn aggancia(destinatario: impl Fn(Comando) + 'static) {
    DESTINATARIO.set(Some(Box::new(destinatario)));
    let aggancio =
        unsafe { SetWindowsHookExW(WH_KEYBOARD, Some(tastiera), None, GetCurrentThreadId()) };
    if let Err(e) = aggancio {
        eprintln!("Scorciatoie da tastiera non disponibili: {e}");
    }
}

fn premuto(tasto: VIRTUAL_KEY) -> bool {
    unsafe { GetKeyState(tasto.0 as i32) < 0 }
}

fn comando(tasto: u16, ctrl: bool, alt: bool) -> Option<Comando> {
    use Comando::*;
    let t = VIRTUAL_KEY(tasto);
    let lettera = |c: char| tasto == c as u16;
    match (ctrl, alt) {
        (false, false) => match t {
            VK_SPACE => Some(PlayPausa),
            VK_DELETE => Some(Cestina),
            VK_MEDIA_STOP => Some(Ferma),
            // I tasti dei vecchi lettori.
            _ if lettera('X') || lettera('C') => Some(PlayPausa),
            _ if lettera('V') => Some(Ferma),
            _ if lettera('B') => Some(Avanti),
            _ if lettera('Z') => Some(Indietro),
            _ => None,
        },
        (true, false) => match t {
            VK_SPACE => Some(PlayPausa),
            VK_OEM_PERIOD => Some(Ferma),
            VK_RIGHT => Some(Avanti),
            VK_LEFT => Some(Indietro),
            VK_UP => Some(VolumeSu),
            VK_DOWN => Some(VolumeGiu),
            _ if lettera('O') => Some(ApriCartella),
            _ if lettera('S') => Some(SalvaPlaylist),
            _ => None,
        },
        (false, true) => match t {
            VK_RIGHT => Some(Avanti),
            VK_LEFT => Some(Indietro),
            _ => None,
        },
        (true, true) => None,
    }
}

unsafe extern "system" fn tastiera(codice: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Bit 31: il tasto è stato rilasciato; bit 30: era già premuto (ripetizione).
    let giu = lparam.0 & (1 << 31) == 0;
    let ripetuto = lparam.0 & (1 << 30) != 0;
    if codice == HC_ACTION as i32 && !SOSPESE.get() {
        if let Some(c) = comando(wparam.0 as u16, premuto(VK_CONTROL), premuto(VK_MENU)) {
            // Il tasto è nostro: non deve anche premere il pulsante che ha il fuoco.
            if giu && (!ripetuto || matches!(c, Comando::VolumeSu | Comando::VolumeGiu)) {
                DESTINATARIO.with_borrow(|d| {
                    if let Some(d) = d {
                        d(c);
                    }
                });
            }
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, codice, wparam, lparam) }
}
