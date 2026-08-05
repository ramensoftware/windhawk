//! What the UI can still say about a window failure the window stack does not
//! report back to it.
//!
//! Tauri runs the `setup` hook from inside the event loop, so a window built
//! there is created through the runtime HANDLE rather than the runtime itself:
//! that path posts the creation to the loop, LOGS a failure through the `log`
//! facade instead of returning it, and hands back a detached window either way.
//! What `build` returns after a failure is then a window that was never
//! registered, whose native window the runtime has already queued for
//! destruction - and since the loop is only asked to exit for a window the
//! runtime knows, nothing reports the loss and nothing ends the process. The
//! window flashes on screen, disappears, and windhawk-ui.exe stays.
//!
//! Two collectors are what make the reason presentable when the UI catches that
//! itself:
//!
//! - [`install_log_capture`] keeps the last few records the stack emits. The
//!   swallowed error goes there, and with it the WebView2 `HRESULT` the failure
//!   came from - which `reported_busy` reads back, since a folder another
//!   process holds is a code only the lost creation could have received.
//! - [`webview_environment_failure`] asks WebView2 to open the folder and reports
//!   what it says, so a missing runtime, a folder that cannot be written, or one
//!   with no space left is named by a code this crate explains in its own words
//!   rather than by another crate's `Display` string.
//!
//! Every cause named here is a code WebView2 returned. Nothing tests the folder
//! on its own to reach a verdict WebView2 did not give, and no code is read as a
//! folder in use unless it means that and nothing else: the browser process this
//! launch started is still alive while the failure is being explained, so both a
//! test for "something holds the profile" and a second environment over the
//! folder answer for the process that is on its way out, and blame another
//! program for it.
//!
//! The probe runs on the failure path alone, so what it costs lands on a launch
//! that is already lost and nowhere else. Both collectors feed
//! [`window_creation_detail`], and the capture alone feeds
//! [`unexpected_close_detail`], which covers the same window going away later in
//! the session (WebView2 destroys its host window when the page or the browser
//! process asks it to) rather than at build.

use std::path::Path;
use std::sync::Mutex;

use webview2_com::Microsoft::Web::WebView2::Win32::{
    CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2EnvironmentOptions,
};
use webview2_com::{CoreWebView2EnvironmentOptions, CreateCoreWebView2EnvironmentCompletedHandler};
use windows_core::{HSTRING, PCWSTR};

/// How many records the capture keeps. Enough to carry the failure line plus the
/// handful around it: this buffer exists to fill in the expander on one fatal
/// dialog, not to be a log.
const KEEP_RECORDS: usize = 8;

/// The records themselves, oldest first, and the most recent error-level one
/// held apart from them.
///
/// The pair is deliberate. A window that fails to build is torn down right
/// after, and the teardown logs too, so the one line that explains the failure
/// is exactly the line a plain ring can lose. Keeping it separately means the
/// details always lead with it, however much noise followed.
static RECORDS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

/// The `log` sink. A unit struct so it can live in a `static`, which is what
/// `log::set_logger` takes.
struct Capture;

static CAPTURE: Capture = Capture;

impl log::Log for Capture {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        // The level filter set below already decides what arrives.
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        let line = format!("{} {}: {}", record.level(), record.target(), record.args());
        if record.level() == log::Level::Error {
            *LAST_ERROR.lock().unwrap_or_else(|error| error.into_inner()) = Some(line.clone());
        }
        push_record(
            &mut RECORDS.lock().unwrap_or_else(|error| error.into_inner()),
            line,
        );
    }

    fn flush(&self) {}
}

/// Start capturing the records the window stack emits.
///
/// Called once, before the app is built. The `log` facade takes one logger per
/// process, and that cuts both ways. A logger installed by anything else (a
/// logging plugin, a test harness) keeps the slot, and the details then come from
/// the WebView2 probe alone - which is why this is best effort. The other
/// direction is the one to know about before reaching for `tauri-plugin-log` or
/// any other sink: this runs first, so from here the slot is TAKEN for the
/// process lifetime, and anything else that wants the records has to be chained
/// off this capture rather than installed beside it.
///
/// The level matters as much as the sink - `log` starts at `Off`, where the
/// macros are no-ops and nothing would ever reach here.
///
/// `Warn` rather than `Info`: what this is for is the failure line, and the
/// stack's chattier levels would only push it out of a ring this small.
pub fn install_log_capture() {
    if log::set_logger(&CAPTURE).is_ok() {
        log::set_max_level(log::LevelFilter::Warn);
    }
}

/// Add `line` to `records`, dropping the oldest once the ring is full.
fn push_record(records: &mut Vec<String>, line: String) {
    if records.len() >= KEEP_RECORDS {
        records.remove(0);
    }
    records.push(line);
}

/// The captured records to show, the pinned error first when the ring no longer
/// holds it (see [`RECORDS`]).
fn tail(records: &[String], last_error: Option<&str>) -> Vec<String> {
    let mut lines = Vec::with_capacity(records.len() + 1);
    if let Some(error) = last_error
        && !records.iter().any(|record| record == error)
    {
        lines.push(error.to_owned());
    }
    lines.extend(records.iter().cloned());
    lines
}

/// Every record captured so far, in the order they are shown.
fn captured() -> Vec<String> {
    let records = RECORDS.lock().unwrap_or_else(|error| error.into_inner());
    let last_error = LAST_ERROR
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    tail(&records, last_error.as_deref())
}

/// Whether any of `lines` carries `code`, in the form the window stack writes an
/// `HRESULT` in: it logs the error's `Debug`, where the code is its own struct
/// field and `windows-result` writes that as `HRESULT(0x800700AA)`.
///
/// One fixed literal, matched case-insensitively, against a formatting that is
/// another crate's to change. A stack that ever writes it differently matches
/// nothing and leaves the diagnosis to the probe - the same answer as a failure
/// that carried no such code, rather than a wrong one.
fn records_carry(lines: &[String], code: i32) -> bool {
    // Both sides upper-cased, the needle included: what is being matched
    // case-insensitively is the whole literal, `0x` prefix and all.
    let needle = format!("HRESULT(0x{code:08X})").to_ascii_uppercase();
    lines
        .iter()
        .any(|line| line.to_ascii_uppercase().contains(&needle))
}

/// Whether the failure the window stack reported was a held data folder.
///
/// The captured records are the only place that code exists on this side: the
/// stack logs its failure rather than returning it (see the module header), so
/// what arrives here is the line rather than the `HRESULT`. Reading it back is
/// what puts the one unambiguous code ([`ERROR_BUSY_HRESULT`]) within reach: it
/// comes from the CONTROLLER creation, and a controller needs the window that has
/// just been lost, so the probe - which creates an environment - is not where it
/// normally arrives.
fn reported_busy() -> bool {
    records_carry(&captured(), ERROR_BUSY_HRESULT)
}

/// A failure WebView2 reported for a data folder, as its `HRESULT` and the
/// system's text for it.
pub struct EnvironmentFailure {
    code: i32,
    message: String,
}

/// Why the window could not be built, as far as this side can tell.
enum Cause {
    /// The failure the window stack reported was a held data folder, by its own
    /// code ([`reported_busy`]).
    Busy,
    /// WebView2 will not open the folder, and said why.
    Environment(EnvironmentFailure),
    /// Neither the reported code nor the probe accounts for it. The captured
    /// records are all there is.
    Unexplained,
}

/// What a failure nothing here accounts for is called. The captured records
/// still carry what the window stack said about it.
const WEBVIEW_NOT_CREATED: &str = "The WebView2 webview it hosts could not be created.";

/// `HRESULT_FROM_WIN32(ERROR_BUSY)`, what a WebView2 controller creation fails
/// with when a browser process already holds the profile in the user data folder.
/// This is the one code that names a held folder and nothing else, whether it
/// arrives from the failure itself ([`reported_busy`]) or from the probe.
const ERROR_BUSY_HRESULT: i32 = 0x8007_00AAu32 as i32;
/// `HRESULT_FROM_WIN32(ERROR_INVALID_STATE)`, which an environment creation fails
/// with when the folder is already open with DIFFERENT environment options.
///
/// A held folder answers this way, and so does one held by the browser process
/// THIS launch started, which is still exiting while the probe runs - and whose
/// options, being wry's rather than the probe's defaults, differ. So it is not
/// read as a folder in use: the case it would have named is the one
/// [`reported_busy`] names from the failure's own code, without having to tell
/// the two holders apart.
const ERROR_INVALID_STATE_HRESULT: i32 = 0x8007_139Fu32 as i32;
/// `E_ACCESSDENIED`: the folder is there and free, and this process may not write
/// to it.
const E_ACCESSDENIED: i32 = 0x8007_0005u32 as i32;
/// `HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND)`, which the loader returns when no
/// WebView2 runtime is installed for this user.
const ERROR_FILE_NOT_FOUND_HRESULT: i32 = 0x8007_0002u32 as i32;
/// `HRESULT_FROM_WIN32(ERROR_DISK_FULL)`: the profile cannot be written out.
const ERROR_DISK_FULL_HRESULT: i32 = 0x8007_0070u32 as i32;

/// Ask WebView2 to open `data_dir` and report what it says, or `None` when it
/// opens it fine.
///
/// This is a diagnosis, not a retry: nothing is kept, and the environment is
/// dropped as soon as the answer is in. Creating an environment is as far as it
/// goes, which is enough for every cause that is about the folder or the runtime
/// rather than about the window (no runtime, an unwritable folder, no disk space,
/// a folder open with other options); what only a controller creation would hit
/// is not reported at all rather than guessed at. A failure that has cleared in
/// between simply answers `None` and leaves the captured records to speak.
///
/// The options are the plain defaults. Configuring them apart from every other
/// environment would make a folder that is open at all answer
/// [`ERROR_INVALID_STATE_HRESULT`] here - including one held by the browser
/// process this launch started, which is still exiting while this runs.
///
/// It runs a message pump of its own while it waits, as every WebView2 creation
/// does. On the failure path that is harmless: the window is already gone or
/// going, and the caller presents and exits.
pub fn webview_environment_failure(data_dir: &Path) -> Option<EnvironmentFailure> {
    let data_directory = HSTRING::from(data_dir);

    let result = CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| {
            let options = CoreWebView2EnvironmentOptions::default();

            // SAFETY: the browser-executable-folder argument is null (use the
            // installed runtime); `data_directory` is a NUL-terminated wide
            // string and the options object is a COM object, both of which
            // outlive the call, and `handler` is the completion handler the
            // wrapper created for this operation. The wrapper owns the wait, so
            // the handler outlives the callback it is invoked through.
            unsafe {
                CreateCoreWebView2EnvironmentWithOptions(
                    PCWSTR::null(),
                    &data_directory,
                    &ICoreWebView2EnvironmentOptions::from(options),
                    &handler,
                )
            }
            .map_err(Into::into)
        }),
        // The completion carries the creation's own result; the environment it
        // may hand over with it is of no interest, and dropping it closes the
        // one this probe opened.
        Box::new(|result, _environment| result),
    );

    match result {
        Ok(()) => None,
        Err(webview2_com::Error::WindowsError(error)) => Some(EnvironmentFailure {
            code: error.code().0,
            message: error.message(),
        }),
        // The wrapper's own failures (a cancelled wait, a send that found no
        // receiver) carry no HRESULT; report them as themselves rather than as a
        // code, so nothing invents one.
        Err(error) => Some(EnvironmentFailure {
            code: 0,
            message: error.to_string(),
        }),
    }
}

/// What to tell the user about `cause`, in their terms: what went wrong with the
/// data folder and what to do about it.
fn explain(cause: &Cause, data_dir: &Path) -> String {
    let folder = data_dir.display();
    let failure = match cause {
        Cause::Busy => return in_use_message(data_dir),
        // Whatever stopped the build is not something this side can account for.
        // The captured records still carry what the window stack said.
        Cause::Unexplained => return WEBVIEW_NOT_CREATED.to_owned(),
        Cause::Environment(failure) => failure,
    };

    match failure.code {
        ERROR_BUSY_HRESULT => in_use_message(data_dir),
        // The folder opened for whoever holds it, so this says nothing about the
        // folder itself, and what it does say cannot be told apart from this
        // launch's own browser process (see the constant).
        ERROR_INVALID_STATE_HRESULT => WEBVIEW_NOT_CREATED.to_owned(),
        E_ACCESSDENIED => format!(
            "Windhawk is not allowed to use its WebView2 data folder:\n\
             {folder}\n\n\
             Check that your user account can write to that folder, then start \
             Windhawk again."
        ),
        ERROR_FILE_NOT_FOUND_HRESULT => {
            "The Microsoft Edge WebView2 Runtime, which draws the Windhawk window, \
             could not be found.\n\n\
             Install it from https://developer.microsoft.com/microsoft-edge/webview2 \
             and start Windhawk again."
                .to_owned()
        }
        ERROR_DISK_FULL_HRESULT => format!(
            "There is no free space left on the drive holding the Windhawk \
             WebView2 data folder:\n\
             {folder}\n\n\
             Free some space, then start Windhawk again."
        ),
        _ => format!(
            "WebView2, which draws the Windhawk window, could not open the \
             Windhawk data folder:\n\
             {folder}"
        ),
    }
}

/// The one message for a data folder something else is holding, whichever code
/// WebView2 reported it with.
fn in_use_message(data_dir: &Path) -> String {
    format!(
        "Another program is already using the Windhawk WebView2 data folder:\n\
         {}\n\n\
         This is usually another Windhawk UI process that has not finished \
         exiting. Open Task Manager, end every \"windhawk-ui.exe\" process on the \
         Details tab, then start Windhawk again.",
        data_dir.display()
    )
}

/// What the probe answered, kept for [`diagnostic_lines`] so the code it found
/// rides with the captured records rather than in what the user reads.
static PROBE_LINE: Mutex<Option<String>> = Mutex::new(None);

/// The raw lines behind a fatal message's expander: what the WebView2 probe found
/// if it has run, then the captured records. `None` when nothing was collected,
/// which is what leaves the expander off the dialog entirely.
pub fn diagnostic_lines() -> Option<String> {
    let probe = PROBE_LINE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();

    let mut lines: Vec<String> = probe.into_iter().collect();
    lines.extend(captured());
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// The probe's answer as one line, in the form the expander shows it.
fn probe_line(failure: &EnvironmentFailure) -> String {
    match failure.code {
        // A wrapper failure carries no HRESULT; report it as itself rather than as
        // a code, so nothing invents one.
        0 => failure.message.clone(),
        code => format!("0x{code:08X}: {}", failure.message.trim()),
    }
}

/// Join a message's paragraphs, dropping the ones that are not there.
fn paragraphs(parts: &[Option<String>]) -> String {
    parts
        .iter()
        .flatten()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Why the window could not be built, on WebView2's word alone: the code the
/// failure itself carried, then what WebView2 answers when it is asked to open
/// the folder. A cause neither reports is left unexplained rather than inferred
/// from the state of the folder, which at this moment still holds this launch's
/// own browser process.
///
/// The reported code first, and not only because it is free: it is the one that
/// came from the attempt that actually failed, so it names a folder in use
/// without the probe having to distinguish that holder from ours - and where it
/// answers, the probe would only start a browser process for nothing.
///
/// A code the probe returns is recorded for [`diagnostic_lines`] on the way past.
fn diagnose(data_dir: &Path) -> Cause {
    if reported_busy() {
        return Cause::Busy;
    }
    match webview_environment_failure(data_dir) {
        Some(failure) => {
            *PROBE_LINE.lock().unwrap_or_else(|error| error.into_inner()) =
                Some(probe_line(&failure));
            Cause::Environment(failure)
        }
        None => Cause::Unexplained,
    }
}

/// What to say about a main window that was handed back but never really built,
/// with `data_dir` the WebView2 data folder it was given. The codes behind it go
/// to [`diagnostic_lines`].
pub fn window_creation_detail(data_dir: &Path) -> String {
    let cause = diagnose(data_dir);

    paragraphs(&[
        Some("The main window could not be created.".to_owned()),
        Some(explain(&cause, data_dir)),
    ])
}

/// What to say about a main window that was destroyed without anyone asking for
/// it to close.
pub fn unexpected_close_detail() -> String {
    paragraphs(&[
        Some(
            "The Windhawk window was closed by the WebView2 component that draws \
             it, which usually means its browser process stopped."
                .to_owned(),
        ),
        Some("Start Windhawk again to continue.".to_owned()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    // The ring is bounded and keeps the RECENT end: what a failure needs is the
    // lines around it, and an unbounded buffer in a process that may run for days
    // is not a buffer.
    #[test]
    fn the_ring_keeps_the_last_records() {
        let mut records = Vec::new();
        for index in 0..KEEP_RECORDS + 3 {
            push_record(&mut records, format!("line {index}"));
        }

        assert_eq!(records.len(), KEEP_RECORDS);
        assert_eq!(records[0], format!("line {}", 3));
        assert_eq!(
            records[KEEP_RECORDS - 1],
            format!("line {}", KEEP_RECORDS + 2)
        );
    }

    // The pinned error is what the whole capture exists for, so it leads the
    // details once the ring has dropped it - the regression being a failure
    // reported with eight teardown lines and no cause.
    #[test]
    fn an_evicted_error_still_leads_the_details() {
        let records: Vec<String> = (0..KEEP_RECORDS).map(|i| format!("line {i}")).collect();

        let lines = tail(&records, Some("ERROR tauri_runtime_wry: failed"));

        assert_eq!(lines[0], "ERROR tauri_runtime_wry: failed");
        assert_eq!(lines.len(), records.len() + 1);
    }

    // And is not repeated when it is still in the ring.
    #[test]
    fn a_retained_error_is_not_repeated() {
        let records = vec!["ERROR x: failed".to_owned(), "WARN x: after".to_owned()];

        let lines = tail(&records, Some("ERROR x: failed"));

        assert_eq!(lines, records);
    }

    // A held folder is the case this whole path started from: it has to name the
    // folder and the way out, not just restate that something failed. Whether the
    // busy code came from the failure itself or from the probe does not change
    // what the user has to do about it.
    #[test]
    fn a_held_folder_is_explained_with_its_remedy() {
        let folder = Path::new(r"C:\Users\test\AppData\Local\Windhawk\UIMainData");
        let reported = explain(&Cause::Busy, folder);
        let probed = explain(
            &Cause::Environment(EnvironmentFailure {
                code: ERROR_BUSY_HRESULT,
                message: "in use".to_owned(),
            }),
            folder,
        );

        assert!(reported.contains(r"C:\Users\test\AppData\Local\Windhawk\UIMainData"));
        assert!(reported.contains("windhawk-ui.exe"));
        assert_eq!(probed, reported);
    }

    // ERROR_INVALID_STATE is what the probe gets over the browser process this
    // launch started as much as over anyone else's, so it accuses nobody: the
    // message is the one for a failure with no account of it, and the code rides
    // in the technical block.
    #[test]
    fn a_folder_open_with_other_options_accuses_nobody() {
        let failure = EnvironmentFailure {
            code: ERROR_INVALID_STATE_HRESULT,
            message: "in use".to_owned(),
        };

        let text = explain(
            &Cause::Environment(failure),
            Path::new(r"C:\Users\test\AppData\Local\Windhawk\UIMainData"),
        );

        assert_eq!(text, explain(&Cause::Unexplained, Path::new(r"C:\data")));
        assert!(!text.contains("windhawk-ui.exe"));
    }

    // The busy code is recognized in the line the window stack logs, since that
    // is the only form it reaches this side in. The failure this all started with
    // (E_INVALIDARG, a webview that could not be created for its own reasons) is
    // the case that must NOT read as a folder in use.
    #[test]
    fn the_busy_code_is_read_back_from_the_captured_line() {
        let busy = r#"ERROR tauri_runtime_wry: failed to create webview: WebView2 error: WindowsError(Error { code: HRESULT(0x800700AA), message: "The requested resource is in use." })"#.to_owned();
        let invalid_arg = r#"ERROR tauri_runtime_wry: failed to create webview: WebView2 error: WindowsError(Error { code: HRESULT(0x80070057), message: "The parameter is incorrect." })"#.to_owned();

        assert!(records_carry(
            &[invalid_arg.clone(), busy.clone()],
            ERROR_BUSY_HRESULT
        ));
        assert!(!records_carry(&[invalid_arg], ERROR_BUSY_HRESULT));
        // The formatting is another crate's; a lower-case code still counts.
        assert!(records_carry(
            &[busy.to_ascii_lowercase()],
            ERROR_BUSY_HRESULT
        ));
    }

    // An unrecognized code still says which folder it was about; the code itself
    // rides in the technical block below it.
    #[test]
    fn an_unknown_code_still_names_the_folder() {
        let failure = EnvironmentFailure {
            code: 0x8000_4005u32 as i32,
            message: "unspecified".to_owned(),
        };

        let text = explain(&Cause::Environment(failure), Path::new(r"C:\data"));

        assert!(text.contains(r"C:\data"));
    }

    // The code the probe found is a line to read back over the phone, so it goes
    // behind the expander in the raw form, not in the explanation.
    #[test]
    fn the_probe_line_carries_the_raw_code() {
        let failure = EnvironmentFailure {
            code: ERROR_INVALID_STATE_HRESULT,
            message: "in use".to_owned(),
        };

        assert_eq!(probe_line(&failure), "0x8007139F: in use");
    }

    // A wrapper failure has no HRESULT of its own, and inventing one (0x00000000)
    // would read as a code that means something.
    #[test]
    fn a_codeless_failure_is_reported_as_itself() {
        let failure = EnvironmentFailure {
            code: 0,
            message: "TaskCanceled".to_owned(),
        };

        assert_eq!(probe_line(&failure), "TaskCanceled");
    }

    // The parts that are absent leave no blank paragraphs behind them.
    #[test]
    fn absent_paragraphs_leave_no_gap() {
        let text = paragraphs(&[Some("first".to_owned()), None, Some("second".to_owned())]);

        assert_eq!(text, "first\n\nsecond");
    }
}
