//! Tauri build hook. It also guarantees a `frontendDist` exists before
//! `tauri::generate_context!` embeds it: the real front-end `dist/` is pulled
//! into the git-ignored `assets/` by the front-end asset sync, but the crate
//! must still compile in a checkout that has not run that sync (CI, a fresh
//! clone). So if `assets/` has no `index.html`, write a minimal placeholder
//! page - enough for the context macro to embed and for the window to load a
//! "not synced" notice. A real sync overwrites it.
//!
//! The log output pane is part of the React front-end bundle (a Monaco viewer
//! in `vscode-windhawk-ui`), staged by the same `assets/` sync as the rest of
//! the app, so there is nothing extra to stage here.

use std::fs;
use std::path::Path;

fn main() {
    ensure_placeholder_assets();
    tauri_build::build();
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
