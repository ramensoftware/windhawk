//! Main-window geometry persistence. Remembers the window's size, on-screen
//! position, and maximized state across runs in a small JSON file the UI owns,
//! so it lands in the Windhawk AppData with the rest of the UI's data (and
//! inside the install tree for a portable copy) rather than at the Tauri
//! default under `%APPDATA%`.
//!
//! The window is the single owner: `run` restores the saved geometry while the
//! window is hidden, then a [`Tracker`] follows move/resize events and writes the
//! latest geometry on close. Tracking is what lets the normal (non-maximized)
//! bounds survive a maximize - while maximized the OS reports the maximized rect,
//! so the tracker keeps the last restored size/position for the next launch and
//! restores it, then re-maximizes.

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

/// The persisted geometry facets: the normal (non-maximized) size and position,
/// plus whether the window was maximized. Size is the inner size in *logical*
/// (DPI-independent) pixels, so the apparent size is preserved when the window
/// reopens under a different display scale; position is the outer position in
/// physical pixels, the coordinate space the monitor bounds and
/// [`WebviewWindow::set_position`] share.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowGeometry {
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    maximized: bool,
}

/// Read the saved geometry. A missing or unreadable/undecodable file is a benign
/// first-run (`None`); the caller keeps the builder defaults.
pub fn load(path: &Path) -> Option<WindowGeometry> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Snapshot the window's current geometry, used to seed the [`Tracker`] on first
/// run so a window maximized-then-closed before any move/resize still persists the
/// real pre-maximize bounds instead of zeros. `None` if the window rejects a read.
pub fn capture(window: &WebviewWindow) -> Option<WindowGeometry> {
    let scale = window.scale_factor().ok()?;
    let size = window.inner_size().ok()?.to_logical::<u32>(scale);
    let position = window.outer_position().ok()?;
    Some(WindowGeometry {
        width: size.width,
        height: size.height,
        x: position.x,
        y: position.y,
        maximized: window.is_maximized().unwrap_or(false),
    })
}

/// Apply saved geometry to the window (called while it is hidden). The logical size
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
pub fn restore(window: &WebviewWindow, geometry: &WindowGeometry) {
    if geometry.width == 0 || geometry.height == 0 {
        return;
    }

    let monitors = window.available_monitors().unwrap_or_default();
    let saved_position = PhysicalPosition::new(geometry.x, geometry.y);
    let saved_size = LogicalSize::new(geometry.width, geometry.height);

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

    if geometry.maximized {
        let _ = window.maximize();
    }
}

/// Follows the live window and persists its geometry on close. Holds the last
/// normal bounds plus the maximized flag; the `run` event handler feeds it
/// move/resize events and calls [`Tracker::save`] on close.
pub struct Tracker {
    state: Mutex<WindowGeometry>,
    path: PathBuf,
}

impl Tracker {
    /// Seed the tracker with the geometry the window opened at (the saved state, or
    /// the current geometry on first run), so a close with no intervening
    /// move/resize still writes sensible bounds.
    pub fn new(path: PathBuf, initial: WindowGeometry) -> Tracker {
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

    /// Capture the current geometry and write it. For a normal window this refreshes
    /// from the live window (covering one never moved or resized since launch); while
    /// maximized/minimized the live values are the maximized rect, so the tracked
    /// normal bounds are kept and only the maximized flag is recorded.
    pub fn save(&self, window: &WebviewWindow) {
        let maximized = window.is_maximized().unwrap_or(false);
        let minimized = window.is_minimized().unwrap_or(false);

        let geometry = {
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

        write(&self.path, &geometry);
    }
}

/// Serialize the geometry to its file, creating the parent directory if needed.
/// Best-effort: a write failure just loses the geometry for next launch, never
/// breaks close.
fn write(path: &Path, geometry: &WindowGeometry) {
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    if let Ok(json) = serde_json::to_vec_pretty(geometry) {
        let _ = std::fs::write(path, json);
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

    fn geometry() -> WindowGeometry {
        WindowGeometry {
            width: 1000,
            height: 700,
            x: 120,
            y: 90,
            maximized: true,
        }
    }

    #[test]
    fn write_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        // The parent (the UI data subfolder) does not exist yet: write must create it.
        let path = dir.path().join("UIMainData").join(FILE_NAME);
        assert!(load(&path).is_none());

        write(&path, &geometry());
        assert_eq!(load(&path), Some(geometry()));
    }

    #[test]
    fn load_ignores_a_corrupt_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        std::fs::write(&path, b"not json").unwrap();
        assert_eq!(load(&path), None);
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
