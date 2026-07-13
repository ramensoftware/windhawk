//! Thin entry point: build and run the Tauri app. All protocol-adapter policy
//! lives in the library crate. The `windows` subsystem suppresses the console
//! window for the GUI in every build.

#![forbid(unsafe_code)]
#![windows_subsystem = "windows"]

fn main() {
    windhawk_ui::run();
}
