//! Main-window state persistence. Remembers the window's size, on-screen
//! position, maximized state, and content zoom factor across runs in a small JSON
//! file the UI owns, so it lands in the Windhawk AppData with the rest of the UI's
//! data (and inside the install tree for a portable copy) rather than at the Tauri
//! default under `%APPDATA%`.
//!
//! The window is the single owner: `run` restores the saved state while the
//! window is hidden, then a [`Tracker`] follows move/resize/zoom events and writes
//! the latest state on close. Tracking is what lets the normal (non-maximized)
//! bounds survive a maximize - while maximized the OS reports the maximized rect,
//! so the tracker keeps the last restored size/position for the next launch and
//! restores it, then re-maximizes.
//!
//! The geometry facets are applied here; the zoom factor is a WebView2 controller
//! property, so `shell` applies it and feeds changes back to the tracker.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{LogicalSize, Monitor, PhysicalPosition, PhysicalSize, WebviewWindow};

/// The file name under the UI data directory ([`crate`]'s `UI_DATA_SUBDIR`).
pub const FILE_NAME: &str = "window-state.json";

/// The window's minimum inner size in logical pixels. Shared with the window
/// builder's `min_inner_size` so the restore clamp and the interactive resize floor
/// stay the same value.
pub const MIN_INNER_WIDTH: u32 = 400;
pub const MIN_INNER_HEIGHT: u32 = 270;

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

/// Apply the saved geometry to the window (called while it is hidden). The logical size
/// is clamped to the configured minimum ([`MIN_INNER_WIDTH`] x [`MIN_INNER_HEIGHT`])
/// and the target display's work area - so it can neither return unusably small nor
/// larger than the screen - then scaled to physical for that display so its apparent
/// size is unchanged under a different display scale (a window shrunk to the minimum
/// at 150% comes back at 400x270, not 600x405, at 100%). The remembered position is
/// reused only when the saved rectangle still lands on a connected monitor; otherwise
/// the window is centered in the work area, so a window saved on a since-removed
/// display cannot return off-screen. A maximized window is restored to its normal
/// bounds first, then maximized, so un-maximizing returns it to the remembered (or
/// centered) size/position.
pub fn restore_geometry(window: &WebviewWindow, state: &WindowState) {
    if state.width == 0 || state.height == 0 {
        return;
    }

    let monitors = window.available_monitors().unwrap_or_default();
    let saved_position = PhysicalPosition::new(state.x, state.y);
    let saved_size = LogicalSize::new(state.width, state.height);

    // The monitor the saved rectangle still lands on, if any: it fixes both the DPI
    // to restore the size at and whether the remembered position is reused.
    let landed = monitors
        .iter()
        .find(|monitor| monitor_covers(monitor, saved_position, saved_size));

    // The display to size and place against: the landed one, else the primary (or
    // any connected) monitor when the saved display is gone.
    let placement = match landed {
        Some(monitor) => Some(monitor.clone()),
        None => window
            .primary_monitor()
            .ok()
            .flatten()
            .or_else(|| monitors.first().cloned()),
    };

    // Move the window onto its destination display BEFORE sizing. The OS rescales a
    // window by the DPI ratio as it crosses monitors, so sizing first and moving after
    // would inflate a window bound for a higher-scale display - a 400 logical width
    // sized on a 100% primary and then moved to a 125% secondary lands as ~500. A
    // still-valid saved position is the final spot; otherwise the window goes to the
    // work-area origin, a provisional spot on the target display, to be centered once
    // its size is known. The window is hidden throughout, so the extra move never shows.
    if let Some(monitor) = placement.as_ref() {
        let landing = if landed.is_some() {
            saved_position
        } else {
            monitor.work_area().position
        };
        let _ = window.set_position(landing);
    }

    // Clamp the logical size to the minimum and the display's work area, then scale to
    // physical for that display's DPI and apply it on the destination monitor.
    let scale = placement
        .as_ref()
        .map(Monitor::scale_factor)
        .unwrap_or_else(|| window.scale_factor().unwrap_or(1.0));
    let work_area = placement.as_ref().map(|monitor| {
        monitor
            .work_area()
            .size
            .to_logical::<u32>(monitor.scale_factor())
    });
    let size = clamp_size(saved_size, work_area);
    let _ = window.set_size(size.to_physical::<u32>(scale));

    // With the size - and thus the outer rectangle - settled on the destination
    // display, center the window in its work area when the saved position was unusable.
    if landed.is_none()
        && let Some(monitor) = placement.as_ref()
    {
        center_in_work_area(window, monitor);
    }

    if state.maximized {
        let _ = window.maximize();
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

/// Center the window's outer rectangle within the monitor's work area (the screen
/// minus the taskbar), the fallback placement when the remembered position is off
/// every connected display. Positions and sizes are physical here, the coordinate
/// space the work area and [`WebviewWindow::outer_size`] share.
fn center_in_work_area(window: &WebviewWindow, monitor: &Monitor) {
    let Ok(outer) = window.outer_size() else {
        return;
    };
    let work_area = monitor.work_area();
    let free_width = (work_area.size.width as i32 - outer.width as i32).max(0);
    let free_height = (work_area.size.height as i32 - outer.height as i32).max(0);
    let position = PhysicalPosition::new(
        work_area.position.x + free_width / 2,
        work_area.position.y + free_height / 2,
    );
    let _ = window.set_position(position);
}

/// Whether any corner of the window rectangle falls inside the monitor - the
/// on-screen test the restore uses to reject a saved position on a display that is
/// no longer connected. The stored logical size is scaled by the monitor's factor to
/// the physical footprint the window would occupy there, then compared against the
/// monitor's physical bounds.
fn monitor_covers(
    monitor: &Monitor,
    window_position: PhysicalPosition<i32>,
    window_size: LogicalSize<u32>,
) -> bool {
    rect_intersects(
        *monitor.position(),
        *monitor.size(),
        window_position,
        window_size.to_physical::<u32>(monitor.scale_factor()),
    )
}

/// The pure geometry behind [`monitor_covers`]: true when any of the window's four
/// corners lies within the monitor's bounds (top/left inclusive, bottom/right
/// exclusive).
fn rect_intersects(
    monitor_position: PhysicalPosition<i32>,
    monitor_size: PhysicalSize<u32>,
    window_position: PhysicalPosition<i32>,
    window_size: PhysicalSize<u32>,
) -> bool {
    let left = monitor_position.x;
    let right = monitor_position.x + monitor_size.width as i32;
    let top = monitor_position.y;
    let bottom = monitor_position.y + monitor_size.height as i32;

    let w = window_size.width as i32;
    let h = window_size.height as i32;
    [
        (window_position.x, window_position.y),
        (window_position.x + w, window_position.y),
        (window_position.x, window_position.y + h),
        (window_position.x + w, window_position.y + h),
    ]
    .into_iter()
    .any(|(x, y)| x >= left && x < right && y >= top && y < bottom)
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

    #[test]
    fn rect_intersects_matches_monitor_bounds() {
        let monitor_position = PhysicalPosition::new(0, 0);
        let monitor_size = PhysicalSize::new(1920, 1080);

        // Fully inside.
        assert!(rect_intersects(
            monitor_position,
            monitor_size,
            PhysicalPosition::new(100, 100),
            PhysicalSize::new(800, 600),
        ));
        // Straddling the right edge: the top-left corner is still on the monitor.
        assert!(rect_intersects(
            monitor_position,
            monitor_size,
            PhysicalPosition::new(1900, 100),
            PhysicalSize::new(800, 600),
        ));
        // Entirely on a disconnected monitor to the left: no corner intersects.
        assert!(!rect_intersects(
            monitor_position,
            monitor_size,
            PhysicalPosition::new(-2000, 100),
            PhysicalSize::new(800, 600),
        ));
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
