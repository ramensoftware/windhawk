//! Main-window state persistence. Remembers the window's size, on-screen
//! position, maximized state, and content zoom factor across runs in a small JSON
//! file the UI owns, so it lands in the Windhawk AppData with the rest of the UI's
//! data (and inside the install tree for a portable copy) rather than at the Tauri
//! default under `%APPDATA%`.
//!
//! The window is the single owner: `run` resolves the saved state into the
//! geometry the window is BUILT at ([`opening_geometry`]) - so it opens where it
//! belongs and can be visible from its first frame - then a [`Tracker`] follows
//! move/resize/zoom events and writes the latest state on close. Tracking is what
//! lets the normal (non-maximized) bounds survive a maximize - while maximized the
//! OS reports the maximized rect, so the tracker keeps the last restored
//! size/position for the next launch and restores it, then re-maximizes.
//!
//! The geometry facets are resolved here; the zoom factor is a WebView2 controller
//! property, so `shell` applies it and feeds changes back to the tracker.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Monitor, PhysicalPosition, PhysicalSize, WebviewWindow,
};

use crate::lifecycle::window::{self, Placement};

/// The file name under the UI data directory ([`crate`]'s `UI_DATA_SUBDIR`).
pub const FILE_NAME: &str = "window-state.json";

/// The window's minimum inner size in logical pixels. Shared with the window
/// builder's `min_inner_size` so the restore clamp and the interactive resize floor
/// stay the same value.
pub const MIN_INNER_WIDTH: u32 = 400;
pub const MIN_INNER_HEIGHT: u32 = 270;

/// The window's inner size on a first run, in logical pixels: the window
/// builder's fallback when there is no saved state, and the size the startup
/// splash opens at (it has no remembered rectangle to match either).
pub const DEFAULT_INNER_SIZE: LogicalSize<u32> = LogicalSize::new(1280, 768);

/// The zoom factor of unzoomed content, and the range a stored one is held to - the
/// 25%-500% browser zoom range, so a corrupt or hand-edited file cannot bring the
/// window back at an unreadable scale (or at a factor WebView2 rejects outright,
/// which is anything at or below zero).
const DEFAULT_ZOOM: f64 = 1.0;
const MIN_ZOOM: f64 = 0.25;
const MAX_ZOOM: f64 = 5.0;

/// The persisted facets: the normal (non-maximized) size and position, whether the
/// window was maximized, and the content zoom factor. Size is the inner size in
/// *logical* (DPI-independent) pixels, so the apparent size is preserved when the
/// window reopens under a different display scale; position is the outer position in
/// physical pixels, the coordinate space the monitor bounds and
/// [`WebviewWindow::set_position`] share. Zoom is independent of the display scale -
/// it is the user's own content scaling on top of it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    maximized: bool,
    // A file written before zoom was persisted decodes as unzoomed rather than
    // failing, which would discard the geometry alongside it.
    #[serde(default = "default_zoom")]
    zoom: f64,
}

impl Default for WindowState {
    fn default() -> WindowState {
        WindowState {
            width: 0,
            height: 0,
            x: 0,
            y: 0,
            maximized: false,
            zoom: DEFAULT_ZOOM,
        }
    }
}

impl WindowState {
    /// The zoom factor to restore the content at, held to the supported range so a
    /// corrupt or hand-edited file cannot apply an unusable one.
    pub fn zoom(&self) -> f64 {
        clamp_zoom(self.zoom)
    }

    /// Whether a size was ever recorded - a state without one carries no geometry
    /// to open at ([`opening_geometry`] falls back to the defaults).
    fn has_geometry(&self) -> bool {
        self.width != 0 && self.height != 0
    }
}

fn default_zoom() -> f64 {
    DEFAULT_ZOOM
}

/// Read the saved state. A missing or unreadable/undecodable file is a benign
/// first-run (`None`); the caller keeps the builder defaults.
pub fn load(path: &Path) -> Option<WindowState> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Snapshot the window's current geometry, used to seed the [`Tracker`] on first
/// run so a window maximized-then-closed before any move/resize still persists the
/// real pre-maximize bounds instead of zeros. `None` if the window rejects a read.
/// The zoom factor is not readable here (it lives on the WebView2 controller), so it
/// starts unzoomed - which is what a fresh webview is - and the tracker takes it from
/// the first zoom change.
pub fn capture(window: &WebviewWindow) -> Option<WindowState> {
    let scale = window.scale_factor().ok()?;
    let size = window.inner_size().ok()?.to_logical::<u32>(scale);
    let position = window.outer_position().ok()?;
    Some(WindowState {
        width: size.width,
        height: size.height,
        x: position.x,
        y: position.y,
        maximized: window.is_maximized().unwrap_or(false),
        zoom: DEFAULT_ZOOM,
    })
}

/// The geometry the window opens at, handed to the window builder so it is created
/// where it belongs rather than moved there afterwards - which is what lets it be
/// visible from the first frame (the startup splash paints inside it while WebView2
/// comes up) instead of hidden until the build returns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OpeningGeometry {
    /// The inner size in logical pixels.
    pub inner_size: LogicalSize<u32>,
    /// The outer position in logical pixels, or `None` to center the window: a
    /// first run, or a remembered position that is no longer on any display.
    pub position: Option<LogicalPosition<f64>>,
    pub maximized: bool,
    /// What the logical values above stand for, in the pixels the displays are
    /// actually laid out in ([`OpeningGeometry::prepare_creation`]). `None` when
    /// there is nothing exact to hold the window to - a first run, or a remembered
    /// position that is no longer on any display, both of which open centered.
    exact: Option<Placement>,
}

impl OpeningGeometry {
    /// Ask for the window to be put on its exact rectangle as it is created, before it
    /// is ever shown, and - for a launch that opens maximized - for it to reach the
    /// screen already maximized ([`window::prepare_main_window_creation`], which is
    /// where the reasons the builder cannot do either are written down). Call just
    /// before the window is built.
    ///
    /// This is what the builder's logical position and size are FOR: they are the same
    /// rectangle expressed in the only coordinates the builder takes, and stand as the
    /// answer whenever this cannot be applied.
    ///
    /// Asked for on every launch, including the ones with no rectangle to hold the
    /// window to: the creation hook also carries the window's activation, which every
    /// launch wants.
    pub fn prepare_creation(&self) {
        window::prepare_main_window_creation(self.exact, self.maximized);
    }

    /// Put the window on its exact rectangle after the fact, for a build that did not
    /// open there - the creation hook could not be installed, or something moved the
    /// window between creating it and returning. Normally a no-op, since the window was
    /// placed as it was created and the builder's own resolution agrees anyway on every
    /// single-scale setup.
    ///
    /// This one IS visible: by the time the build returns the window has been on screen
    /// for as long as WebView2 took to come up. It is the fallback, not the mechanism.
    ///
    /// Skipped for a window that opened maximized, where a move is not meaningful: its
    /// position and size are the display it was maximized onto. Its pre-maximize
    /// rectangle is the creation hook's to have got right.
    pub fn place_exactly(&self, window: &WebviewWindow) {
        let Some(exact) = self.exact.filter(|_| !self.maximized) else {
            return;
        };
        // Move before sizing: crossing to a display at another scale makes Windows
        // rescale the window by the DPI ratio, so a size applied first would land
        // inflated or shrunk by exactly that ratio.
        if window.outer_position().is_ok_and(|at| at != exact.position) {
            let _ = window.set_position(exact.position);
        }
        if window
            .inner_size()
            .is_ok_and(|size| size != exact.inner_size)
        {
            let _ = window.set_size(exact.inner_size);
        }
    }
}

/// Resolve where the window should open from the saved state and the connected
/// displays.
///
/// The logical size is clamped to the configured minimum ([`MIN_INNER_WIDTH`] x
/// [`MIN_INNER_HEIGHT`]) and the target display's work area, so it can neither
/// return unusably small nor larger than the screen. Keeping it LOGICAL is what
/// preserves its apparent size under a different display scale (a window shrunk to
/// the minimum at 150% comes back at 400x270, not 600x405, at 100%): the builder
/// scales it for the display the position selects. The remembered position is
/// reused only when the saved rectangle still meets a connected monitor;
/// otherwise the window is centered, so a window saved on a since-removed display
/// cannot return off-screen. A maximized window carries its normal bounds too, so
/// un-maximizing returns it to the remembered (or centered) size and position.
pub fn opening_geometry(app: &AppHandle, saved: Option<&WindowState>) -> OpeningGeometry {
    let displays: Vec<Display> = app
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(Display::from)
        .collect();
    let primary = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|monitor| Display::from(&monitor));
    resolve_opening_geometry(&displays, primary, saved)
}

/// One connected display, as the placement rules see it. Taken from Tauri's
/// [`Monitor`] so the rules themselves are testable without a running app.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Display {
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    work_area: PhysicalSize<u32>,
    scale_factor: f64,
}

impl From<&Monitor> for Display {
    fn from(monitor: &Monitor) -> Display {
        Display {
            position: *monitor.position(),
            size: *monitor.size(),
            work_area: monitor.work_area().size,
            scale_factor: monitor.scale_factor(),
        }
    }
}

/// The placement rules, over plain display descriptions (see [`opening_geometry`]).
fn resolve_opening_geometry(
    displays: &[Display],
    primary: Option<Display>,
    saved: Option<&WindowState>,
) -> OpeningGeometry {
    let default = OpeningGeometry {
        inner_size: DEFAULT_INNER_SIZE,
        position: None,
        maximized: false,
        exact: None,
    };
    let Some(state) = saved.filter(|state| state.has_geometry()) else {
        return default;
    };

    let saved_position = PhysicalPosition::new(state.x, state.y);
    let saved_size = LogicalSize::new(state.width, state.height);

    // The display the saved rectangle belongs to, if any: the one it covers most of,
    // which is the display Windows itself puts a window on. It decides both the scale
    // the size is held against and whether the remembered position is reused.
    //
    // Most of it, not any of it. A window resting against the left edge of its display
    // sits at x = -7 - the invisible resize border is outside the frame - so it reaches
    // seven pixels onto whatever is to the left of it. Handing it to the display it
    // barely touches hands it that display's scale, which the remembered size is
    // converted by, and the window comes back at the wrong size on the right display.
    let landed = displays
        .iter()
        .map(|display| (display.overlap(saved_position, saved_size), display))
        .filter(|(overlap, _)| *overlap > 0)
        // Strictly greater, so a tie keeps the earlier display rather than the later.
        .reduce(|best, next| if next.0 > best.0 { next } else { best })
        .map(|(_, display)| display);
    // The display to size against: the landed one, else the primary (or any
    // connected) display when the saved one is gone.
    let placement = landed
        .copied()
        .or(primary)
        .or_else(|| displays.first().copied());

    let work_area =
        placement.map(|display| display.work_area.to_logical::<u32>(display.scale_factor));
    let inner_size = clamp_size(saved_size, work_area);

    OpeningGeometry {
        inner_size,
        position: landed.map(|display| saved_position.to_logical(display.scale_factor)),
        maximized: state.maximized,
        // Only where the window is held to a remembered spot - which is also the only
        // case where `placement` is the landed display, so the size below is scaled by
        // the display the position puts the window on. A centered window has no exact
        // rectangle to hold it to, and is sized against the primary display either
        // way, which is the same one the builder resolves to.
        exact: landed.map(|display| Placement {
            position: saved_position,
            inner_size: inner_size.to_physical(display.scale_factor),
        }),
    }
}

/// Follows the live window and persists its state on close. Holds the last normal
/// bounds, the maximized flag, and the content zoom factor; the `run` event handler
/// feeds it move/resize events, `shell`'s zoom subscription feeds it zoom changes,
/// and the close handler calls [`Tracker::save`].
pub struct Tracker {
    state: Mutex<WindowState>,
    path: PathBuf,
}

impl Tracker {
    /// Seed the tracker with the state the window opened at (the saved state, or
    /// the current geometry on first run), so a close with no intervening
    /// move/resize/zoom still writes sensible values.
    pub fn new(path: PathBuf, initial: WindowState) -> Tracker {
        Tracker {
            state: Mutex::new(initial),
            path,
        }
    }

    /// Record a move. The maximized/minimized states report the maximized origin
    /// (or a stale position), so update the stored position only for a normal
    /// window; the maximized flag is always refreshed.
    pub fn on_moved(&self, window: &WebviewWindow, position: PhysicalPosition<i32>) {
        let maximized = window.is_maximized().unwrap_or(false);
        let minimized = window.is_minimized().unwrap_or(false);
        let mut state = self.state.lock().unwrap();
        state.maximized = maximized;
        if !maximized && !minimized {
            state.x = position.x;
            state.y = position.y;
        }
    }

    /// Record a resize, mirroring [`Tracker::on_moved`] for the size: a maximize
    /// resizes to the monitor, which must not overwrite the normal size.
    pub fn on_resized(&self, window: &WebviewWindow, size: PhysicalSize<u32>) {
        let maximized = window.is_maximized().unwrap_or(false);
        let minimized = window.is_minimized().unwrap_or(false);
        let size = size.to_logical::<u32>(window.scale_factor().unwrap_or(1.0));
        let mut state = self.state.lock().unwrap();
        state.maximized = maximized;
        if !maximized && !minimized && size.width > 0 && size.height > 0 {
            state.width = size.width;
            state.height = size.height;
        }
    }

    /// Record a zoom change - the user's Ctrl+/-/0, Ctrl+wheel, or pinch on the
    /// content. Unlike the bounds this needs no maximized/minimized guard: the zoom
    /// factor is the user's own content scaling, independent of the window's size and
    /// state. The value is held to the accepted range, so what lands in the file is
    /// always something the next run can apply.
    pub fn on_zoom_changed(&self, zoom: f64) {
        let mut state = self.state.lock().unwrap();
        state.zoom = clamp_zoom(zoom);
    }

    /// Capture the current geometry and write the state. For a normal window this
    /// refreshes from the live window (covering one never moved or resized since
    /// launch); while maximized/minimized the live values are the maximized rect, so
    /// the tracked normal bounds are kept and only the maximized flag is recorded. The
    /// zoom factor is written as tracked - it is not readable from the window here.
    pub fn save(&self, window: &WebviewWindow) {
        let maximized = window.is_maximized().unwrap_or(false);
        let minimized = window.is_minimized().unwrap_or(false);

        let saved = {
            let mut state = self.state.lock().unwrap();
            state.maximized = maximized;
            if !maximized && !minimized {
                if let Ok(position) = window.outer_position() {
                    state.x = position.x;
                    state.y = position.y;
                }
                if let Ok(size) = window.inner_size() {
                    let size = size.to_logical::<u32>(window.scale_factor().unwrap_or(1.0));
                    if size.width > 0 && size.height > 0 {
                        state.width = size.width;
                        state.height = size.height;
                    }
                }
            }
            *state
        };

        write(&self.path, &saved);
    }
}

/// Serialize the state to its file, creating the parent directory if needed.
/// Best-effort: a write failure just loses the state for next launch, never
/// breaks close.
fn write(path: &Path, state: &WindowState) {
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    if let Ok(json) = serde_json::to_vec_pretty(state) {
        let _ = std::fs::write(path, json);
    }
}

/// Hold a zoom factor to the supported range, mapping a non-finite one (a NaN or
/// infinity a hand-edited file can carry) to unzoomed.
fn clamp_zoom(zoom: f64) -> f64 {
    if zoom.is_finite() {
        zoom.clamp(MIN_ZOOM, MAX_ZOOM)
    } else {
        DEFAULT_ZOOM
    }
}

/// Clamp a restored logical inner size to the usable range: never below the
/// configured minimum ([`MIN_INNER_WIDTH`] x [`MIN_INNER_HEIGHT`]), and never above
/// the target display's work area when it is known. Capping to the work area is
/// applied first and the minimum second, so on a display smaller than the minimum the
/// minimum wins (the window stays usable rather than being pinned below its floor).
fn clamp_size(size: LogicalSize<u32>, work_area: Option<LogicalSize<u32>>) -> LogicalSize<u32> {
    let (max_width, max_height) =
        work_area.map_or((u32::MAX, u32::MAX), |area| (area.width, area.height));
    LogicalSize::new(
        size.width.min(max_width).max(MIN_INNER_WIDTH),
        size.height.min(max_height).max(MIN_INNER_HEIGHT),
    )
}

impl Display {
    /// How much of the window rectangle falls on this display - what decides which
    /// display a remembered rectangle belongs to, and so whether the remembered
    /// position is on screen at all. The stored logical size is scaled by this
    /// display's factor to the physical footprint the window would occupy here, then
    /// intersected with its physical bounds.
    fn overlap(
        &self,
        window_position: PhysicalPosition<i32>,
        window_size: LogicalSize<u32>,
    ) -> i64 {
        rect_overlap(
            self.position,
            self.size,
            window_position,
            window_size.to_physical::<u32>(self.scale_factor),
        )
    }
}

/// The pure geometry behind [`Display::overlap`]: the area, in physical pixels, that
/// the monitor and the window rectangle share - zero when they do not meet at all.
/// Widened to `i64`, which the product of two screen-sized spans cannot overflow.
fn rect_overlap(
    monitor_position: PhysicalPosition<i32>,
    monitor_size: PhysicalSize<u32>,
    window_position: PhysicalPosition<i32>,
    window_size: PhysicalSize<u32>,
) -> i64 {
    /// The length the two spans share on one axis (top/left inclusive, bottom/right
    /// exclusive), which is zero when they miss each other.
    fn shared(a_start: i32, a_length: u32, b_start: i32, b_length: u32) -> i64 {
        let (a_start, b_start) = (i64::from(a_start), i64::from(b_start));
        let start = a_start.max(b_start);
        let end = (a_start + i64::from(a_length)).min(b_start + i64::from(b_length));
        (end - start).max(0)
    }

    shared(
        monitor_position.x,
        monitor_size.width,
        window_position.x,
        window_size.width,
    ) * shared(
        monitor_position.y,
        monitor_size.height,
        window_position.y,
        window_size.height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> WindowState {
        WindowState {
            width: 1000,
            height: 700,
            x: 120,
            y: 90,
            maximized: true,
            zoom: 1.25,
        }
    }

    #[test]
    fn write_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        // The parent (the UI data subfolder) does not exist yet: write must create it.
        let path = dir.path().join("UIMainData").join(FILE_NAME);
        assert!(load(&path).is_none());

        write(&path, &state());
        assert_eq!(load(&path), Some(state()));
    }

    #[test]
    fn load_ignores_a_corrupt_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        std::fs::write(&path, b"not json").unwrap();
        assert_eq!(load(&path), None);
    }

    #[test]
    fn load_accepts_a_file_without_a_zoom_factor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        std::fs::write(
            &path,
            br#"{"width":1000,"height":700,"x":120,"y":90,"maximized":true}"#,
        )
        .unwrap();

        // The geometry survives and the missing zoom reads as unzoomed, rather than
        // the whole file being rejected as undecodable.
        let loaded = load(&path).expect("a file without zoom still decodes");
        assert_eq!(
            loaded,
            WindowState {
                zoom: DEFAULT_ZOOM,
                ..state()
            },
        );
    }

    #[test]
    fn zoom_is_held_to_the_supported_range() {
        // In range: applied as stored.
        assert_eq!(state().zoom(), 1.25);
        // Out of range in either direction: clamped to the range's edge.
        assert_eq!(clamp_zoom(0.01), MIN_ZOOM);
        assert_eq!(clamp_zoom(50.0), MAX_ZOOM);
        // Not a finite factor at all (a hand-edited file): unzoomed.
        assert_eq!(clamp_zoom(f64::NAN), DEFAULT_ZOOM);
        assert_eq!(clamp_zoom(f64::INFINITY), DEFAULT_ZOOM);
    }

    // A first run has no file, and a window that never zooms writes the default: the
    // seed must be unzoomed, not the 0.0 a derived Default would give.
    #[test]
    fn the_default_state_is_unzoomed() {
        assert_eq!(WindowState::default().zoom(), DEFAULT_ZOOM);
    }

    fn display(x: i32, y: i32, width: u32, height: u32, scale_factor: f64) -> Display {
        Display {
            position: PhysicalPosition::new(x, y),
            size: PhysicalSize::new(width, height),
            // A work area one taskbar shorter than the display, as a real one is.
            work_area: PhysicalSize::new(width, height - 40),
            scale_factor,
        }
    }

    // The common case: the remembered rectangle still lands on a connected
    // display, so the window opens exactly where it was closed, at the size it
    // was, converted to the logical coordinates the builder takes.
    #[test]
    fn a_remembered_rectangle_opens_where_it_was() {
        let displays = [display(0, 0, 1920, 1080, 1.0)];
        let opening = resolve_opening_geometry(&displays, Some(displays[0]), Some(&state()));

        assert_eq!(opening.inner_size, LogicalSize::new(1000, 700));
        assert_eq!(opening.position, Some(LogicalPosition::new(120.0, 90.0)));
        assert!(opening.maximized);
    }

    // The position is physical and the builder's is logical, so a display running
    // at a scale converts it - and the size, being logical already, is unchanged
    // (the window keeps its apparent size there).
    #[test]
    fn a_scaled_display_converts_the_position_and_keeps_the_logical_size() {
        let displays = [display(0, 0, 2400, 1350, 1.5)];
        let opening = resolve_opening_geometry(&displays, Some(displays[0]), Some(&state()));

        assert_eq!(opening.position, Some(LogicalPosition::new(80.0, 60.0)));
        assert_eq!(opening.inner_size, LogicalSize::new(1000, 700));
    }

    // A rectangle saved on a display that is no longer connected must not come
    // back off-screen: it loses its position (the builder centers it) but keeps
    // its size, clamped to the display it will actually open on.
    #[test]
    fn a_disconnected_display_drops_the_position_and_clamps_to_the_remaining_one() {
        let remaining = display(0, 0, 1280, 800, 1.0);
        let saved = WindowState {
            x: -3000,
            y: 200,
            ..state()
        };
        let opening = resolve_opening_geometry(&[remaining], Some(remaining), Some(&saved));

        assert_eq!(opening.position, None);
        // 700 does not fit the 760-tall work area... it does; the width is what the
        // display constrains here, so only the height survives untouched.
        assert_eq!(opening.inner_size, LogicalSize::new(1000, 700));

        // A window larger than the remaining display is capped to its work area.
        let large = WindowState {
            width: 4000,
            height: 3000,
            ..saved
        };
        let opening = resolve_opening_geometry(&[remaining], Some(remaining), Some(&large));
        assert_eq!(opening.inner_size, LogicalSize::new(1280, 760));
    }

    // A first run (no file) and a state that never recorded a size both open at
    // the default size, centered - with nothing exact to hold them to.
    #[test]
    fn no_saved_geometry_opens_at_the_default_size_centered() {
        let displays = [display(0, 0, 1920, 1080, 1.0)];
        let default = OpeningGeometry {
            inner_size: DEFAULT_INNER_SIZE,
            position: None,
            maximized: false,
            exact: None,
        };

        assert_eq!(
            resolve_opening_geometry(&displays, Some(displays[0]), None),
            default
        );
        assert_eq!(
            resolve_opening_geometry(&displays, Some(displays[0]), Some(&WindowState::default())),
            default
        );
    }

    // With several displays connected, the one the saved rectangle lands on is the
    // one that decides the scale - not the primary.
    #[test]
    fn the_landed_display_decides_the_scale() {
        let primary = display(0, 0, 1920, 1080, 1.0);
        let secondary = display(1920, 0, 2560, 1440, 2.0);
        let saved = WindowState {
            x: 2400,
            y: 200,
            ..state()
        };

        let opening = resolve_opening_geometry(&[primary, secondary], Some(primary), Some(&saved));
        // 2400 physical on a 200% display is 1200 logical.
        assert_eq!(opening.position, Some(LogicalPosition::new(1200.0, 100.0)));
    }

    // The logical position the builder takes is ambiguous across displays at
    // different scales - (1200, 100) is the saved spot on the 200% secondary AND a
    // spot on the 100% primary, and tao resolves it against whichever display it
    // matches first. So the rectangle is carried in the pixels the displays are laid
    // out in as well, and re-applied once the window is built.
    #[test]
    fn a_remembered_rectangle_is_carried_in_physical_pixels_too() {
        let primary = display(0, 0, 1920, 1080, 1.0);
        let secondary = display(1920, 0, 2560, 1440, 2.0);
        let saved = WindowState {
            x: 2400,
            y: 200,
            ..state()
        };

        let opening = resolve_opening_geometry(&[primary, secondary], Some(primary), Some(&saved));
        assert_eq!(
            opening.exact,
            Some(Placement {
                // Exactly where it was closed, not the logical value re-scaled.
                position: PhysicalPosition::new(2400, 200),
                // 1000x700 logical at the secondary's 200%.
                inner_size: PhysicalSize::new(2000, 1400),
            })
        );
    }

    // A window that opens centered has no remembered spot to be held to, and is
    // sized against the same display the builder resolves to, so there is nothing to
    // re-apply - only a rectangle the builder could place wrong carries one.
    #[test]
    fn a_centered_window_carries_no_exact_rectangle() {
        let remaining = display(0, 0, 1280, 800, 1.0);
        let off_screen = WindowState {
            x: -3000,
            y: 200,
            ..state()
        };

        assert_eq!(
            resolve_opening_geometry(&[remaining], Some(remaining), Some(&off_screen)).exact,
            None
        );
        assert_eq!(
            resolve_opening_geometry(&[remaining], Some(remaining), None).exact,
            None
        );
    }

    // A window resting against the left edge of its display reaches a few pixels
    // onto the display beside it. It belongs to the one it is actually on, since
    // whichever display wins decides the scale its remembered size is converted by -
    // and the neighbour running at another scale is where that goes visibly wrong.
    #[test]
    fn a_sliver_over_a_border_does_not_hand_the_window_to_the_neighbour() {
        let left = display(-2560, 0, 2560, 1440, 1.5);
        let main = display(0, 0, 1920, 1080, 1.0);
        let saved = WindowState {
            width: 1093,
            height: 659,
            x: -7,
            y: 0,
            maximized: false,
            zoom: DEFAULT_ZOOM,
        };

        let opening = resolve_opening_geometry(&[left, main], Some(main), Some(&saved));

        // The main display's scale, so the position comes back exactly as written and
        // the size is the one that was saved rather than that size at 150%.
        assert_eq!(opening.position, Some(LogicalPosition::new(-7.0, 0.0)));
        assert_eq!(opening.inner_size, LogicalSize::new(1093, 659));
        assert_eq!(
            opening.exact,
            Some(Placement {
                position: PhysicalPosition::new(-7, 0),
                inner_size: PhysicalSize::new(1093, 659),
            })
        );
    }

    #[test]
    fn rect_overlap_measures_the_shared_area() {
        let monitor_position = PhysicalPosition::new(0, 0);
        let monitor_size = PhysicalSize::new(1920, 1080);
        let overlap = |x, y, width, height| {
            rect_overlap(
                monitor_position,
                monitor_size,
                PhysicalPosition::new(x, y),
                PhysicalSize::new(width, height),
            )
        };

        // Fully inside: the window's own area.
        assert_eq!(overlap(100, 100, 800, 600), 800 * 600);
        // Straddling the right edge: only the part on the monitor counts.
        assert_eq!(overlap(1900, 100, 800, 600), 20 * 600);
        // Entirely on a disconnected monitor to the left: nothing shared.
        assert_eq!(overlap(-2000, 100, 800, 600), 0);
        // Touching the edge from outside is not overlapping it.
        assert_eq!(overlap(-800, 100, 800, 600), 0);
    }

    #[test]
    fn clamp_size_respects_min_and_work_area() {
        let work_area = Some(LogicalSize::new(1920, 1040));

        // Within range: unchanged.
        assert_eq!(
            clamp_size(LogicalSize::new(1000, 700), work_area),
            LogicalSize::new(1000, 700),
        );
        // Below the minimum: floored to it.
        assert_eq!(
            clamp_size(LogicalSize::new(100, 100), work_area),
            LogicalSize::new(MIN_INNER_WIDTH, MIN_INNER_HEIGHT),
        );
        // Larger than the work area: capped to fit the screen.
        assert_eq!(
            clamp_size(LogicalSize::new(4000, 3000), work_area),
            LogicalSize::new(1920, 1040),
        );
        // No work area known: only the minimum floor applies.
        assert_eq!(
            clamp_size(LogicalSize::new(100, 100), None),
            LogicalSize::new(MIN_INNER_WIDTH, MIN_INNER_HEIGHT),
        );
        // Work area smaller than the minimum: the minimum wins, no panic.
        assert_eq!(
            clamp_size(
                LogicalSize::new(1000, 700),
                Some(LogicalSize::new(200, 150))
            ),
            LogicalSize::new(MIN_INNER_WIDTH, MIN_INNER_HEIGHT),
        );
    }
}
