//! The clipboard-history shim: put the webview's copies back on the clipboard under
//! this window's ownership, so Windows records them in clipboard history (Win+V).
//!
//! Windows attributes each clipboard item to the window that owns it, and the history
//! service skips the ones owned by WebView2's internal browser-process window. Nothing
//! copied in the app reaches Win+V as a result - by any route, since every route ends in
//! the same owner: Ctrl+C, the trimmed context menu's Copy, and the front-end's own copy
//! buttons alike. Ordinary Ctrl+V paste is unaffected throughout, which is what makes the
//! gap easy to miss. WebView2 has no switch for this, and the content itself is well
//! formed - text, HTML, and Chromium's private formats, carrying none of the
//! `ExcludeClipboardContentFromMonitorProcessing` opt-out markers - so ownership is the
//! only thing left to change.
//!
//! [`keep_copies_in_history`] watches the clipboard from the main window and, when a
//! change turns out to be owned by this app's own webview, writes the same payloads back
//! with the main window as owner. That second write is the one history records.
//!
//! Three things keep the rewrite from doing damage:
//!
//! - **Scope.** Only a clipboard owned by a `msedgewebview2.exe` process descended from
//!   THIS process is rewritten. Matching the image name alone would re-own copies made
//!   in every other WebView2 app on the machine - Widgets, Teams, another Tauri app -
//!   and leave Win+V crediting them to Windhawk.
//! - **Fidelity.** Every format is duplicated byte for byte, including Chromium's private
//!   ones, and the whole set is allocated before `EmptyClipboard` is called, so a format
//!   that cannot be read or copied leaves the original contents alone. Emptying is the
//!   point of no return: past it the originals are freed, and a block the system then
//!   declines is dropped from the set rather than restored. Formats whose handle is not a
//!   memory block cannot be duplicated that way at all; a clipboard carrying one is left
//!   alone entirely rather than rewritten without it (losing data is worse than losing a
//!   history entry).
//! - **Termination.** The rewrite makes the main window the owner, which fails the
//!   descended-from-us test, so the `WM_CLIPBOARDUPDATE` it raises in turn is ignored.
//!
//! No OS-version gate: the supported WebView2 runtime floor is at or past the Windows 10
//! build that introduced clipboard history, so the rewrite is never dead weight.

use std::collections::HashMap;
use std::sync::mpsc::{SyncSender, sync_channel};
use std::thread;
use std::time::Duration;

use tauri::WebviewWindow;
use windows_sys::Win32::Foundation::{
    CloseHandle, GlobalFree, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, EnumClipboardFormats,
    GetClipboardData, GetClipboardOwner, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, WM_CLIPBOARDUPDATE};

/// The WebView2 browser process, which is the one that owns the clipboard after a copy
/// in the webview.
const WEBVIEW_PROCESS: &str = "msedgewebview2.exe";

/// How far up the parent chain the descended-from-us test walks. The browser process is
/// a direct child of this one; the couple of extra rungs cost nothing and cover a
/// runtime that grows an intermediate process.
const MAX_ANCESTRY_DEPTH: usize = 4;

/// Attempts to open the clipboard before giving up on a rewrite, and the pause between
/// them. Whoever else has it open holds it for a single read or write, so a handful of
/// short retries covers the contention; a clipboard that stays busy just leaves the copy
/// as WebView2 wrote it.
const OPEN_ATTEMPTS: u32 = 10;
const OPEN_RETRY: Duration = Duration::from_millis(10);

/// Record this window's webview copies in Windows clipboard history, by rewriting them
/// under this window's ownership (see the module docs for why they are missed otherwise).
///
/// The clipboard listener and the subclass that receives its `WM_CLIPBOARDUPDATE` live
/// for the window's lifetime (the app has a single window, open until exit), which is
/// what makes leaking the boxed sender sound and lets the registration go unrecorded -
/// the listener is dropped with the window.
///
/// The detection runs on the window thread; the rewrite itself runs on a worker thread,
/// so waiting for another app to let go of the clipboard never stalls the UI. Best
/// effort throughout: a window without a handle, a subclass the system declines, or a
/// clipboard that stays busy leaves the copy as WebView2 wrote it, which is the behavior
/// without this shim.
pub fn keep_copies_in_history(window: &WebviewWindow) {
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let hwnd = hwnd.0;

    // Capacity 1: while a rewrite is in flight the next change only needs to leave a
    // single "look again" behind, and a full channel means one is already pending.
    let (updates, pending) = sync_channel::<()>(1);
    // A raw HWND is a pointer and not `Send`, so the worker takes the address and casts
    // it back. The window outlives the thread (both run until the process exits), and the
    // clipboard calls the worker makes with it are thread-agnostic: `OpenClipboard` takes
    // any window as the incoming owner, and the payloads it writes are real memory
    // blocks, so nothing is ever rendered back on the owner's thread.
    let owner = hwnd as usize;
    thread::spawn(move || {
        while pending.recv().is_ok() {
            reown(owner as HWND);
        }
    });

    let updates = Box::into_raw(Box::new(updates));
    // SAFETY: `hwnd` is the live main window and this runs on the thread that owns it,
    // as subclassing requires. `updates` is a live pointer to a leaked box the subclass
    // procedure only ever borrows, and it outlives the window. A failure leaves the box
    // leaked and nothing subclassed, which is harmless.
    let subclassed = unsafe {
        SetWindowSubclass(
            hwnd,
            Some(clipboard_proc),
            CLIPBOARD_SUBCLASS_ID,
            updates as usize,
        )
    };
    if subclassed == 0 {
        return;
    }

    // Registered after the subclass, so the first update cannot arrive before there is
    // anything to receive it.
    //
    // SAFETY: `hwnd` is the live main window; the listener is dropped along with it.
    unsafe {
        AddClipboardFormatListener(hwnd);
    }
}

/// The subclass id for [`keep_copies_in_history`]. Windows keys a subclass on the
/// procedure and id together, so this only has to stay distinct from the other id this
/// crate uses (`shell::ACTIVATION_SUBCLASS_ID`), against the day both hooks share one
/// procedure.
const CLIPBOARD_SUBCLASS_ID: usize = 2;

/// The [`keep_copies_in_history`] subclass: nudge the worker on each clipboard change,
/// then let the message take its normal course.
unsafe extern "system" fn clipboard_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    reference: usize,
) -> LRESULT {
    if message == WM_CLIPBOARDUPDATE {
        // SAFETY: `reference` is the pointer `keep_copies_in_history` leaked for this
        // window, which outlives it; the box is only borrowed here.
        let updates = unsafe { &*(reference as *const SyncSender<()>) };
        // A full channel is a rewrite already queued, which will see this change too.
        let _ = updates.try_send(());
    }
    // SAFETY: the arguments are the ones the subclass procedure was handed, passed on to
    // the rest of the chain.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

/// Rewrite the current clipboard contents under `hwnd`'s ownership, if they came from
/// this app's webview. Anything else - this window's own rewrite included - is left
/// untouched.
fn reown(hwnd: HWND) {
    // The cheap half of the test first, so a copy made in another app costs neither a
    // process snapshot nor contention over opening the clipboard.
    let Some(owner) = clipboard_owner_pid() else {
        return;
    };

    let Some(_open) = Clipboard::open(hwnd) else {
        return;
    };
    // The owner may have changed while we waited for the clipboard; with it open, what
    // this reads is what the duplication below will read.
    if clipboard_owner_pid() != Some(owner) || !is_our_webview(owner) {
        return;
    }

    let formats = clipboard_formats();
    let Some(payloads) = duplicate_all(&formats) else {
        return;
    };

    // SAFETY: the clipboard is open (the guard above). EmptyClipboard frees the previous
    // contents - already duplicated into `payloads` - and makes `hwnd` the owner, which
    // is the whole point of the rewrite.
    if unsafe { EmptyClipboard() } == 0 {
        discard(payloads);
        return;
    }

    // With the originals freed there is nothing to fall back to, so a block the system
    // declines costs its own format rather than the rest of the set.
    for payload in payloads {
        // SAFETY: the clipboard is open and was emptied by us, so this process may place
        // data on it. `handle` is a moveable block allocated by `duplicate_all` and not
        // otherwise referenced; on success the system takes ownership of it.
        let placed = unsafe { SetClipboardData(payload.format, payload.handle) };
        if placed.is_null() {
            // SAFETY: the system did not take the block, so it is still ours to free and
            // nothing else refers to it.
            unsafe {
                GlobalFree(payload.handle);
            }
        }
    }
}

/// The clipboard held open for the lifetime of the value, with the window passed to
/// [`Clipboard::open`] as the one `EmptyClipboard` will make the owner.
struct Clipboard;

impl Clipboard {
    /// Open the clipboard, retrying briefly while another process holds it.
    fn open(hwnd: HWND) -> Option<Self> {
        for attempt in 0..OPEN_ATTEMPTS {
            // SAFETY: `hwnd` is the live main window. OpenClipboard either associates the
            // clipboard with it and succeeds, or fails because someone else has it open;
            // it has no other effect.
            if unsafe { OpenClipboard(hwnd) } != 0 {
                return Some(Self);
            }
            if attempt + 1 < OPEN_ATTEMPTS {
                thread::sleep(OPEN_RETRY);
            }
        }
        None
    }
}

impl Drop for Clipboard {
    fn drop(&mut self) {
        // SAFETY: this value exists only for a clipboard this thread opened, and Drop
        // runs once, so the close is paired with exactly one successful open.
        unsafe {
            CloseClipboard();
        }
    }
}

/// A clipboard payload duplicated out of the open clipboard, ready to be placed back.
/// `handle` is a moveable memory block this module allocated: either the system takes
/// ownership of it through `SetClipboardData`, or it is freed by [`discard`].
struct Payload {
    format: u32,
    handle: HANDLE,
}

/// Every format on the open clipboard, in the order it offers them (the order they were
/// placed, which is the priority order a paste target walks).
fn clipboard_formats() -> Vec<u32> {
    let mut formats = Vec::new();
    let mut format = 0;
    loop {
        // SAFETY: the clipboard is open (the caller holds the guard).
        // EnumClipboardFormats walks on from the format handed in and answers 0 at the
        // end (and on error).
        format = unsafe { EnumClipboardFormats(format) };
        if format == 0 {
            return formats;
        }
        formats.push(format);
    }
}

/// Duplicate every format worth carrying over, or `None` if the set cannot be reproduced
/// faithfully - in which case nothing stays allocated and the clipboard is left alone.
///
/// The whole set is built before the caller empties the clipboard, so a failure here
/// costs the history entry rather than the copy.
fn duplicate_all(formats: &[u32]) -> Option<Vec<Payload>> {
    let mut payloads = Vec::with_capacity(formats.len());
    for &format in formats {
        if is_synthesized_from(format, formats) {
            continue;
        }
        if is_opaque_handle(format) {
            discard(payloads);
            return None;
        }
        let Some(payload) = duplicate(format) else {
            discard(payloads);
            return None;
        };
        payloads.push(payload);
    }
    // An empty set would leave the clipboard cleared rather than rewritten.
    (!payloads.is_empty()).then_some(payloads)
}

/// Copy one format's bytes into a fresh moveable block, or `None` if the data cannot be
/// read or the allocation fails.
fn duplicate(format: u32) -> Option<Payload> {
    // SAFETY: the clipboard is open (the caller holds the guard). GetClipboardData
    // answers a handle owned by the clipboard - borrowed here, never freed - or null. The
    // format is one EnumClipboardFormats just offered, and `is_opaque_handle` has ruled
    // out the standard formats whose handle is not a memory block, so what comes back is
    // a moveable block the Global* calls below accept.
    let source = unsafe { GetClipboardData(format) };
    if source.is_null() {
        return None;
    }

    // SAFETY: `source` is a moveable block (see above). GlobalSize answers its byte count,
    // and GlobalLock either pins it and answers a pointer to those bytes or answers null.
    let (bytes, read) = unsafe { (GlobalSize(source), GlobalLock(source)) };
    if bytes == 0 || read.is_null() {
        if !read.is_null() {
            // SAFETY: paired with the successful lock just above.
            unsafe {
                GlobalUnlock(source);
            }
        }
        return None;
    }

    // SAFETY: a moveable allocation of the same size, answering null on failure, locked
    // for the copy below.
    let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) };
    let write = if handle.is_null() {
        std::ptr::null_mut()
    } else {
        // SAFETY: `handle` is the block just allocated, and nothing else holds it.
        unsafe { GlobalLock(handle) }
    };
    if write.is_null() {
        // SAFETY: `source` is still locked from above. `handle`, if it was allocated at
        // all, is ours alone and unlocked, so freeing it releases the failed attempt.
        unsafe {
            GlobalUnlock(source);
            if !handle.is_null() {
                GlobalFree(handle);
            }
        }
        return None;
    }

    // SAFETY: both blocks are locked, and `bytes` is the source's own size, which the
    // destination was allocated to match. The two allocations do not overlap.
    unsafe {
        std::ptr::copy_nonoverlapping(read.cast::<u8>(), write.cast::<u8>(), bytes);
        GlobalUnlock(handle);
        GlobalUnlock(source);
    }

    Some(Payload { format, handle })
}

/// Free payloads that will not be placed on the clipboard.
fn discard(payloads: Vec<Payload>) {
    for payload in payloads {
        // SAFETY: each block was allocated here and never handed to the system, so this
        // is the only reference to it.
        unsafe {
            GlobalFree(payload.handle);
        }
    }
}

/// Whether a format is one Windows will regenerate from another format in the same set,
/// making it redundant to carry over. Only the text and bitmap conversions are claimed,
/// and each only when a format it is derived from is present alongside it.
fn is_synthesized_from(format: u32, formats: &[u32]) -> bool {
    const CF_TEXT: u32 = 1;
    const CF_BITMAP: u32 = 2;
    const CF_OEMTEXT: u32 = 7;
    const CF_DIB: u32 = 8;
    const CF_UNICODETEXT: u32 = 13;
    const CF_DIBV5: u32 = 17;

    let sources: &[u32] = match format {
        CF_TEXT | CF_OEMTEXT => &[CF_UNICODETEXT],
        CF_BITMAP => &[CF_DIB, CF_DIBV5],
        _ => return false,
    };
    sources.iter().any(|source| formats.contains(source))
}

/// Whether a format's handle is something other than a memory block, and so cannot be
/// duplicated by copying bytes: the GDI-object and owner-drawn standard formats, the
/// display variants of each, and the private and GDI-object ranges an app defines for
/// itself. A clipboard carrying one of these is left alone rather than rewritten without
/// it. Registered formats (`0xC000` and up) - which is what the webview's HTML and
/// Chromium's private formats are - are always memory blocks.
fn is_opaque_handle(format: u32) -> bool {
    const CF_BITMAP: u32 = 2;
    const CF_METAFILEPICT: u32 = 3;
    const CF_PALETTE: u32 = 9;
    const CF_ENHMETAFILE: u32 = 14;
    const CF_OWNERDISPLAY: u32 = 0x0080;
    const CF_DSPBITMAP: u32 = 0x0082;
    const CF_DSPMETAFILEPICT: u32 = 0x0083;
    const CF_DSPENHMETAFILE: u32 = 0x008E;
    const CF_PRIVATE: std::ops::RangeInclusive<u32> = 0x0200..=0x02FF;
    const CF_GDIOBJ: std::ops::RangeInclusive<u32> = 0x0300..=0x03FF;

    matches!(
        format,
        CF_BITMAP
            | CF_METAFILEPICT
            | CF_PALETTE
            | CF_ENHMETAFILE
            | CF_OWNERDISPLAY
            | CF_DSPBITMAP
            | CF_DSPMETAFILEPICT
            | CF_DSPENHMETAFILE
    ) || CF_PRIVATE.contains(&format)
        || CF_GDIOBJ.contains(&format)
}

/// The process behind the current clipboard owner, or `None` when there is no owner
/// window or the owner is this process - which is what a rewrite of ours leaves behind,
/// and what stops it from feeding itself.
fn clipboard_owner_pid() -> Option<u32> {
    // SAFETY: GetClipboardOwner takes no arguments and only reads the current owner,
    // answering null when there is none. The handle is passed on, not dereferenced.
    let owner = unsafe { GetClipboardOwner() };
    if owner.is_null() {
        return None;
    }
    let mut pid = 0;
    // SAFETY: `owner` is the handle just read; the pid is written through the
    // out-pointer, and left at 0 for a window that has gone away.
    unsafe { GetWindowThreadProcessId(owner, &mut pid) };

    // SAFETY: GetCurrentProcessId takes no arguments and cannot fail.
    let ours = unsafe { GetCurrentProcessId() };
    (pid != 0 && pid != ours).then_some(pid)
}

/// Whether `pid` is a WebView2 browser process belonging to THIS app: named
/// [`WEBVIEW_PROCESS`] and descended from this process.
///
/// Both halves are needed. The name alone would claim every other WebView2 app's copies;
/// the ancestry alone rests on parent ids, which Windows reuses once a parent exits, so
/// the name is what keeps a recycled id from matching.
fn is_our_webview(pid: u32) -> bool {
    // SAFETY: GetCurrentProcessId takes no arguments and cannot fail.
    let ours = unsafe { GetCurrentProcessId() };
    let processes = snapshot_processes();
    if !processes
        .get(&pid)
        .is_some_and(|process| process.name.eq_ignore_ascii_case(WEBVIEW_PROCESS))
    {
        return false;
    }

    let mut current = pid;
    for _ in 0..MAX_ANCESTRY_DEPTH {
        let Some(parent) = processes.get(&current).map(|process| process.parent) else {
            return false;
        };
        if parent == ours {
            return true;
        }
        // A parent id that is gone, or that points back at the process itself, ends the
        // walk rather than looping it.
        if parent == 0 || parent == current {
            return false;
        }
        current = parent;
    }
    false
}

/// One row of [`snapshot_processes`]: what the ancestry walk needs about a process.
struct Process {
    parent: u32,
    name: String,
}

/// Snapshot every running process's id -> parent id and exe name.
fn snapshot_processes() -> HashMap<u32, Process> {
    let mut processes = HashMap::new();
    // SAFETY: a process snapshot of the whole system; INVALID_HANDLE_VALUE on failure.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return processes;
    }
    // PROCESSENTRY32W is a C POD; dwSize must be set before the first enumeration call.
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    // SAFETY: snapshot is valid; entry is sized.
    let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) };
    while ok != 0 {
        processes.insert(
            entry.th32ProcessID,
            Process {
                parent: entry.th32ParentProcessID,
                name: wide_buf_to_string(&entry.szExeFile),
            },
        );
        // SAFETY: same valid snapshot and entry.
        ok = unsafe { Process32NextW(snapshot, &mut entry) };
    }
    // SAFETY: the snapshot handle is closed exactly once.
    unsafe { CloseHandle(snapshot) };
    processes
}

/// Decode a NUL-terminated UTF-16 buffer (`PROCESSENTRY32W::szExeFile`).
fn wide_buf_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The formats a webview text/HTML copy puts on the clipboard, in the order it
    /// offers them: the registered HTML and Chromium ones, CF_UNICODETEXT, CF_LOCALE,
    /// and the CF_TEXT/CF_OEMTEXT pair Windows synthesizes.
    const WEBVIEW_TEXT_COPY: &[u32] = &[49378, 13, 49935, 49938, 49939, 16, 1, 7];

    #[test]
    fn a_webview_text_copy_is_carried_over_whole() {
        assert!(!WEBVIEW_TEXT_COPY.iter().any(|&f| is_opaque_handle(f)));
        let carried: Vec<u32> = WEBVIEW_TEXT_COPY
            .iter()
            .copied()
            .filter(|&f| !is_synthesized_from(f, WEBVIEW_TEXT_COPY))
            .collect();
        // The two text formats Windows regenerates from CF_UNICODETEXT drop out; the
        // HTML, the unicode text, CF_LOCALE and Chromium's private formats are kept.
        assert_eq!(carried, vec![49378, 13, 49935, 49938, 49939, 16]);
    }

    #[test]
    fn synthesized_formats_drop_out_only_beside_their_source() {
        assert!(is_synthesized_from(1, &[1, 13]));
        assert!(!is_synthesized_from(1, &[1, 16]));
        assert!(is_synthesized_from(2, &[2, 8]));
        assert!(!is_synthesized_from(2, &[2]));
        assert!(!is_synthesized_from(13, &[1, 13]));
    }

    #[test]
    fn gdi_and_app_defined_formats_abandon_the_rewrite() {
        assert!(is_opaque_handle(2));
        assert!(is_opaque_handle(3));
        assert!(is_opaque_handle(9));
        assert!(is_opaque_handle(0x0080));
        assert!(is_opaque_handle(0x0250));
        assert!(is_opaque_handle(0x0350));
        assert!(!is_opaque_handle(13));
        assert!(!is_opaque_handle(15));
        assert!(!is_opaque_handle(49378));
    }

    #[test]
    fn an_image_copy_is_abandoned_when_only_the_gdi_handle_is_offered() {
        // CF_BITMAP alongside CF_DIB is Windows' own conversion and drops out; on its
        // own it is the only copy of the image, and being an opaque handle it abandons
        // the rewrite rather than losing it.
        assert!(is_synthesized_from(2, &[2, 8]));
        assert!(!is_synthesized_from(2, &[2]));
        assert!(is_opaque_handle(2));
    }
}
