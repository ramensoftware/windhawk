//! Tauri build hook. It also guarantees a `frontendDist` exists before
//! `tauri::generate_context!` embeds it: the real front-end `dist/` is pulled
//! into the git-ignored `assets/` by the front-end asset sync, but the crate
//! must still compile in a checkout that has not run that sync (CI, a fresh
//! clone). So if `assets/` has no `index.html`, write a minimal placeholder
//! page - enough for the context macro to embed and for the window to load a
//! "not synced" notice. A real sync overwrites it.
//!
//! The log output pane is part of the React front-end bundle (a Monaco viewer
//! in `windhawk-frontend`), staged by the same `assets/` sync as the rest of
//! the app, so there is nothing extra to stage here.
//!
//! The third piece is the application manifest ([`embed_app_manifest`]), which
//! the linker embeds in the app executable and the test binaries alike.

use std::env;
use std::fs;
use std::path::Path;

use tauri_build::{Attributes, WindowsAttributes};

fn main() {
    ensure_placeholder_assets();
    embed_app_manifest();
    // Two manifests in one binary is a link error.
    let attributes =
        Attributes::new().windows_attributes(WindowsAttributes::new_without_app_manifest());
    tauri_build::try_build(attributes).expect("tauri-build failed");
}

/// Write a placeholder `assets/index.html` when the synced front-end is absent,
/// so `generate_context!` always has a `frontendDist` to embed.
fn ensure_placeholder_assets() {
    let assets = Path::new("assets");
    let index = assets.join("index.html");
    if index.exists() {
        return;
    }
    // Best-effort: a failure here surfaces as the context macro's own missing-dir
    // error, which is the clearer message.
    let _ = fs::create_dir_all(assets);
    let _ = fs::write(
        &index,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Windhawk</title></head>\
         <body><p>The Windhawk UI front-end assets have not been synced into this \
         build.</p></body></html>\n",
    );
}

/// The application manifest: the common-controls dependency, and nothing else.
const APP_MANIFEST: &str = "app-manifest.xml";

/// Have the linker embed [`APP_MANIFEST`] in every binary this crate links.
///
/// A binary's manifest is what decides which comctl32 it binds to, and this crate calls
/// entry points only version 6 has: `LoadIconWithScaleDown` for the window icons
/// (shell.rs), `TaskDialogIndirect` for the fatal- and stuck-startup dialogs
/// (lifecycle/window.rs). A binary bound to System32's comctl32 (5.82) instead does not
/// fail at the call: it fails to LOAD, as `STATUS_ENTRYPOINT_NOT_FOUND`, before the
/// first test runs and with nothing said about which entry point.
///
/// tauri-build would embed the same manifest through its resource file, but cargo links
/// that into binaries only, which leaves out the library's own unit tests - the lib built
/// with `--test`, which no per-kind link-arg form names. The general form below reaches
/// every target.
fn embed_app_manifest() {
    let manifest =
        Path::new(&env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"))
            .join(APP_MANIFEST);
    println!("cargo:rerun-if-changed={}", manifest.display());

    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    // Embed it as written; the linker otherwise adds an `asInvoker` trustInfo section of
    // its own.
    println!("cargo:rustc-link-arg=/MANIFESTUAC:NO");
}
