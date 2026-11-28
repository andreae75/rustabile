//! Una sola finestra, come la versione GTK: chi apre un altro file mentre
//! Rustabile è già aperto lo passa a quella finestra (`WM_COPYDATA`) ed esce.

use std::cell::RefCell;
use std::time::Duration;

use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentThreadId};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumThreadWindows, EnumWindows, GetClassNameW, GetPropW, IsIconic, SMTO_ABORTIFHUNG,
    SW_RESTORE, SendMessageTimeoutW, SetForegroundWindow, SetPropW, ShowWindow, WM_COPYDATA,
};
use windows::core::{BOOL, PCWSTR, w};

/// Il segno che riconosce la nostra finestra fra quelle di Windows.
const SEGNO: PCWSTR = w!("Rustabile.WinUI.Finestra");
/// Distingue i nostri messaggi da altri `WM_COPYDATA`.
const MESSAGGIO: usize = 0x5255_5354;

type Destinatario = Box<dyn Fn(Vec<String>)>;

thread_local! {
    static DESTINATARIO: RefCell<Option<Destinatario>> = const { RefCell::new(None) };
}

/// All'avvio: se Rustabile è già aperto gli passa `uri`, lo porta davanti e
/// restituisce `true` (questo processo deve uscire).
pub fn passata_a_quella_aperta(uri: &[String]) -> bool {
    // Il mutex resta nostro finché il processo vive: la maniglia non si chiude.
    let mutex = unsafe { CreateMutexW(None, false, w!("Local\\Rustabile.WinUI")) };
    if mutex.is_err() || unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
        return false;
    }
    // Aprendo più file insieme, la prima finestra può non essere ancora pronta.
    for _ in 0..100 {
        if let Some(finestra) = cerca_finestra() {
            return invia(finestra, uri);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn cerca_finestra() -> Option<HWND> {
    unsafe extern "system" fn guarda(finestra: HWND, trovata: LPARAM) -> BOOL {
        unsafe {
            if GetPropW(finestra, SEGNO).is_invalid() {
                return true.into();
            }
            *(trovata.0 as *mut Option<HWND>) = Some(finestra);
        }
        false.into()
    }
    let mut trovata: Option<HWND> = None;
    // Fermare l'elenco a metà risulta come errore: non conta.
    let _ = unsafe { EnumWindows(Some(guarda), LPARAM(&mut trovata as *mut _ as isize)) };
    trovata
}

fn invia(finestra: HWND, uri: &[String]) -> bool {
    let testo = uri.join("\n");
    let dati = COPYDATASTRUCT {
        dwData: MESSAGGIO,
        cbData: testo.len() as u32,
        lpData: testo.as_ptr() as *mut _,
    };
    unsafe {
        if IsIconic(finestra).as_bool() {
            let _ = ShowWindow(finestra, SW_RESTORE);
        }
        let _ = SetForegroundWindow(finestra);
        let mut risposta = 0;
        let inviato = SendMessageTimeoutW(
            finestra,
            WM_COPYDATA,
            WPARAM(0),
            LPARAM(&dati as *const _ as isize),
            SMTO_ABORTIFHUNG,
            5000,
            Some(&mut risposta),
        );
        inviato.0 != 0 && risposta == 1
    }
}

/// Sul thread della finestra, quando esiste: da qui riceve i file degli altri avvii.
pub fn aggancia(destinatario: impl Fn(Vec<String>) + 'static) {
    DESTINATARIO.set(Some(Box::new(destinatario)));
    unsafe {
        let _ = EnumThreadWindows(GetCurrentThreadId(), Some(segna), LPARAM(0));
    }
}

unsafe extern "system" fn segna(finestra: HWND, _: LPARAM) -> BOOL {
    let mut classe = [0u16; 64];
    let n = unsafe { GetClassNameW(finestra, &mut classe) } as usize;
    if String::from_utf16_lossy(&classe[..n]) == "WinUIDesktopWin32WindowClass" {
        unsafe {
            if SetWindowSubclass(finestra, Some(ricevi), 1, 0).as_bool() {
                let _ = SetPropW(
                    finestra,
                    SEGNO,
                    Some(HANDLE(std::ptr::without_provenance_mut(1))),
                );
            }
        }
    }
    true.into()
}

unsafe extern "system" fn ricevi(
    finestra: HWND,
    messaggio: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    if messaggio == WM_COPYDATA {
        let dati = unsafe { &*(lparam.0 as *const COPYDATASTRUCT) };
        if dati.dwData == MESSAGGIO {
            let byte = if dati.cbData == 0 {
                &[][..]
            } else {
                unsafe {
                    std::slice::from_raw_parts(dati.lpData as *const u8, dati.cbData as usize)
                }
            };
            let uri: Vec<String> = String::from_utf8_lossy(byte)
                .lines()
                .map(str::to_string)
                .collect();
            DESTINATARIO.with_borrow(|d| {
                if let Some(d) = d {
                    d(uri);
                }
            });
            return LRESULT(1);
        }
    }
    unsafe { DefSubclassProc(finestra, messaggio, wparam, lparam) }
}
