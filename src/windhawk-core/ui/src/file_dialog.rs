//! Native Win32 file dialogs for user-data export/import. The host owns the archive
//! file I/O, so these run inside the `wh_ipc` worker: export opens a Save picker
//! for the archive it writes, and inspect/import an Open picker for the archive
//! it reads. Each enters a single-threaded COM apartment, shows an `IFileDialog`
//! parented to the main window, and reports the chosen path, a user cancel, or a
//! shell failure.

use std::ffi::c_void;
use std::path::PathBuf;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
    CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FileOpenDialog, FileSaveDialog, IFileOpenDialog, IFileSaveDialog, IShellItem, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
use windows::core::{HRESULT, HSTRING, PCWSTR, PWSTR, w};

use crate::lifecycle::window::MAIN_WINDOW_CLASS;

/// `HRESULT_FROM_WIN32(ERROR_CANCELLED)`: the `IFileDialog::Show` result when the
/// user dismisses the picker - a benign no-op, told apart from a real failure.
const ERROR_CANCELLED_HRESULT: HRESULT = HRESULT(0x8007_04C7u32 as i32);

/// `RPC_E_CHANGED_MODE`: `CoInitializeEx` rejecting STA because the thread is already
/// an MTA. The dialog still works, so we proceed - just without owning the uninit.
const RPC_E_CHANGED_MODE: HRESULT = HRESULT(0x8001_0106u32 as i32);

/// The outcome of showing a file dialog.
pub enum DialogOutcome {
    /// The user chose a path.
    Picked(PathBuf),
    /// The user dismissed the dialog (a benign no-op, not an error).
    Canceled,
    /// The dialog could not be shown (a COM/shell failure), with a diagnostic.
    Failed(String),
}

/// The native file dialogs the user-data handlers reach through
/// [`BridgeCtx`](crate::ipc::bridge::BridgeCtx). Injected as a trait so the handlers
/// are headless-testable (a fake returns a canned path or a cancel); the production
/// [`Win32FileDialog`] shows the real pickers. `Send + Sync` so the context that holds
/// it can cross to the `wh_ipc` worker thread.
pub trait FileDialog: Send + Sync {
    /// Show a Save picker for the exported archive, seeded with `default_name`.
    fn save_archive(&self, default_name: &str) -> DialogOutcome;
    /// Show an Open picker for an archive to inspect/import.
    fn open_archive(&self) -> DialogOutcome;
}

/// The production dialogs: the native Win32 `IFileSaveDialog` / `IFileOpenDialog`.
pub struct Win32FileDialog;

impl FileDialog for Win32FileDialog {
    fn save_archive(&self, default_name: &str) -> DialogOutcome {
        save_dialog(default_name)
    }
    fn open_archive(&self) -> DialogOutcome {
        open_dialog()
    }
}

/// Show a Save picker for the exported archive, seeded with `default_name`.
fn save_dialog(default_name: &str) -> DialogOutcome {
    let Some(_com) = ComApartment::enter() else {
        return DialogOutcome::Failed("COM initialization failed".to_owned());
    };
    // SAFETY: the standard IFileSaveDialog sequence; the dialog and shell item are
    // released on drop, and the display-name PWSTR is freed by `result_path`.
    unsafe {
        let dialog: IFileSaveDialog =
            match CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER) {
                Ok(dialog) => dialog,
                Err(e) => return DialogOutcome::Failed(format!("CoCreateInstance: {e}")),
            };
        let filters = json_filters();
        let _ = dialog.SetFileTypes(&filters);
        let _ = dialog.SetDefaultExtension(w!("json"));
        let _ = dialog.SetFileName(&HSTRING::from(default_name));
        shown(dialog.Show(main_window_owner()), || dialog.GetResult())
    }
}

/// Show an Open picker for an archive to inspect/import.
fn open_dialog() -> DialogOutcome {
    let Some(_com) = ComApartment::enter() else {
        return DialogOutcome::Failed("COM initialization failed".to_owned());
    };
    // SAFETY: the standard IFileOpenDialog sequence; see `save_archive`.
    unsafe {
        let dialog: IFileOpenDialog =
            match CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) {
                Ok(dialog) => dialog,
                Err(e) => return DialogOutcome::Failed(format!("CoCreateInstance: {e}")),
            };
        let filters = json_filters();
        let _ = dialog.SetFileTypes(&filters);
        shown(dialog.Show(main_window_owner()), || dialog.GetResult())
    }
}

/// Map a dialog `Show` result to the outcome: a cancel HRESULT is [`DialogOutcome::Canceled`],
/// any other error is [`DialogOutcome::Failed`], and success resolves the result item to a
/// filesystem path.
///
/// # Safety
/// `get_result` must call `IFileDialog::GetResult` on the dialog that produced
/// `show`, and is invoked only after a successful `Show`.
unsafe fn shown(
    show: windows::core::Result<()>,
    get_result: impl FnOnce() -> windows::core::Result<IShellItem>,
) -> DialogOutcome {
    match show {
        Ok(()) => unsafe { result_path(get_result()) },
        Err(e) if e.code() == ERROR_CANCELLED_HRESULT => DialogOutcome::Canceled,
        Err(e) => DialogOutcome::Failed(format!("Show: {e}")),
    }
}

/// Resolve a chosen `IShellItem` to its filesystem path, freeing the display-name
/// buffer the shell allocates.
///
/// # Safety
/// Calls COM methods on `item` and reads/frees the returned `PWSTR`; safe when
/// `item` is a live shell item from a successful `GetResult`.
unsafe fn result_path(item: windows::core::Result<IShellItem>) -> DialogOutcome {
    let item = match item {
        Ok(item) => item,
        Err(e) => return DialogOutcome::Failed(format!("GetResult: {e}")),
    };
    let pwstr: PWSTR = match unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) } {
        Ok(pwstr) => pwstr,
        Err(e) => return DialogOutcome::Failed(format!("GetDisplayName: {e}")),
    };
    let path = unsafe { pwstr.to_string() };
    // SAFETY: `pwstr` is the CoTaskMem buffer GetDisplayName just allocated.
    unsafe { CoTaskMemFree(Some(pwstr.0 as *const c_void)) };
    match path {
        Ok(path) => DialogOutcome::Picked(PathBuf::from(path)),
        Err(e) => DialogOutcome::Failed(format!("path decode: {e}")),
    }
}

/// The picker's file-type filter: the archive's `.json`, plus an all-files escape.
fn json_filters() -> [COMDLG_FILTERSPEC; 2] {
    [
        COMDLG_FILTERSPEC {
            pszName: w!("Windhawk user data (*.json)"),
            pszSpec: w!("*.json"),
        },
        COMDLG_FILTERSPEC {
            pszName: w!("All files (*.*)"),
            pszSpec: w!("*.*"),
        },
    ]
}

/// The main window handle to own the modal dialog, or `None` (an ownerless dialog)
/// when the window is not found - so a picker still shows if the lookup fails.
fn main_window_owner() -> Option<HWND> {
    // SAFETY: FindWindowW reads window state; a null window-name matches any title. A
    // missing window is `Err` (a null handle), which `.ok()` turns into `None`.
    unsafe { FindWindowW(&HSTRING::from(MAIN_WINDOW_CLASS), PCWSTR::null()) }.ok()
}

/// A COM apartment held for the lifetime of a dialog. Entering initializes the
/// worker thread as STA (the modal picker needs it); `owned` tracks whether this
/// guard performed the init, so `Drop` balances it - a thread already in an MTA
/// (`RPC_E_CHANGED_MODE`) is used as-is and left untouched.
struct ComApartment {
    owned: bool,
}

impl ComApartment {
    fn enter() -> Option<ComApartment> {
        // SAFETY: CoInitializeEx is always safe to call; STA suits the modal dialog.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        if hr.is_ok() {
            Some(ComApartment { owned: true })
        } else if hr == RPC_E_CHANGED_MODE {
            Some(ComApartment { owned: false })
        } else {
            None
        }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: balanced with the successful CoInitializeEx on this thread.
            unsafe { CoUninitialize() };
        }
    }
}
