//! L'icona della finestra. Quella del file .exe la mette `build.rs`, ma una
//! finestra WinUI non la usa da sola: gliela diamo noi.

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumThreadWindows, GetSystemMetrics, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_SHARED, LoadImageW,
    SM_CXICON, SM_CXSMICON, SendMessageW, WM_SETICON,
};
use windows::core::{BOOL, PCWSTR};

/// Il numero con cui `winresource` registra l'icona.
const RISORSA: usize = 1;

/// Da chiamare sul thread della finestra, quando la finestra esiste.
pub fn applica() {
    unsafe {
        let _ = EnumThreadWindows(GetCurrentThreadId(), Some(una_finestra), LPARAM(0));
    }
}

unsafe extern "system" fn una_finestra(finestra: HWND, _: LPARAM) -> BOOL {
    unsafe {
        let Ok(modulo) = GetModuleHandleW(None) else {
            return false.into();
        };
        for (tipo, misura) in [(ICON_SMALL, SM_CXSMICON), (ICON_BIG, SM_CXICON)] {
            let lato = GetSystemMetrics(misura);
            if let Ok(icona) = LoadImageW(
                Some(modulo.into()),
                PCWSTR(RISORSA as *const u16),
                IMAGE_ICON,
                lato,
                lato,
                LR_SHARED,
            ) {
                SendMessageW(
                    finestra,
                    WM_SETICON,
                    Some(WPARAM(tipo as usize)),
                    Some(LPARAM(icona.0 as isize)),
                );
            }
        }
    }
    true.into()
}
