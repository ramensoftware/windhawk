//! Build hook: embed a Windows VERSIONINFO resource into the cdylib so the
//! shipped windhawk-core.dll carries the app version and the Ramen Software /
//! Windhawk product strings in its Details tab, matching the version info the
//! Tauri UI already embeds in windhawk-ui.exe. The .rc is generated from Cargo's
//! version env vars and compiled by the Windows SDK RC.EXE via embed-resource.

use std::env;
use std::fs;
use std::path::Path;

// windhawk-core.dll is a library, so it declares VFT_DLL. Its FileDescription
// distinguishes it from the CLI and the UI, which share the "Windhawk" product.
const FILE_TYPE: &str = "0x2L"; // VFT_DLL
const FILE_DESCRIPTION: &str = "Windhawk Core";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let rc_path = Path::new(&out_dir).join("windhawk_version.rc");
    fs::write(&rc_path, version_resource()).expect("failed to write the version resource");

    // The crate is a cdylib with no binaries, so embed-resource links the
    // compiled resource into the cdylib itself. manifest_required() turns a
    // missing RC.EXE or a failed compile into a hard build error, so a shipped
    // DLL never silently loses its version info.
    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_required()
        .expect("failed to embed the version resource");
}

/// Render the VERSIONINFO .rc text from the crate version. The numeric
/// FILEVERSION/PRODUCTVERSION tuple stays strictly numeric (major.minor.patch.0)
/// while the pre-release tag rides along only in the FileVersion/ProductVersion
/// strings (e.g. "2.0.0-alpha.1"), matching tauri-winres.
fn version_resource() -> String {
    let major = env::var("CARGO_PKG_VERSION_MAJOR").unwrap();
    let minor = env::var("CARGO_PKG_VERSION_MINOR").unwrap();
    let patch = env::var("CARGO_PKG_VERSION_PATCH").unwrap();
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    // VS_FF_DEBUG in dev builds; shipped release artifacts carry no flags.
    let file_flags = if env::var("PROFILE").as_deref() == Ok("release") {
        "0x0L"
    } else {
        "0x1L"
    };
    let file_type = FILE_TYPE;
    let description = FILE_DESCRIPTION;

    // 040904b0: US English (0x0409), Unicode code page (0x04b0 = 1200).
    format!(
        r#"1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEFLAGSMASK 0x3fL
FILEFLAGS {file_flags}
FILEOS 0x40004L
FILETYPE {file_type}
FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "Ramen Software"
            VALUE "FileDescription", "{description}"
            VALUE "FileVersion", "{version}"
            VALUE "ProductName", "Windhawk"
            VALUE "ProductVersion", "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
    )
}
