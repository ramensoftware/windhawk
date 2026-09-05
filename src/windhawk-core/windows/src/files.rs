//! The `Files` port adapter: a thin wrapper over `std::fs`, with atomic replace
//! through `MoveFileExW` (`MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH`)
//! so external readers never observe a half-written file (the user profile).
//! Read/list/exists/mtime are plain std calls; the atomic rename and the private
//! temp directory's security descriptor are what need Win32.

use std::io;
use std::io::Write;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::GENERIC_WRITE;
use windows_sys::Win32::Storage::FileSystem::{
    CreateDirectoryW, DELETE, FILE_FLAG_DELETE_ON_CLOSE, MOVEFILE_REPLACE_EXISTING,
    MOVEFILE_WRITE_THROUGH, MoveFileExW,
};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

use windhawk_core_ports::{DirEntry, FileError, FileErrorKind, Files};

use crate::security::{
    DirAttributes, DirSecurity, is_elevated, owner_sid_string, private_dir_security,
    released_dir_sddl, set_protected_dacl,
};
use crate::wide::path_to_wide;

pub struct WindowsFiles;

/// Counter mixed into generated file and directory names so two concurrent
/// calls in one process never collide before the create-must-be-new check even
/// runs. A process-local atomic is fine here - this is the sanctioned Win32
/// adapter crate, outside the no-globals gate.
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn kind_of(e: &io::Error) -> FileErrorKind {
    if e.kind() == io::ErrorKind::NotFound {
        FileErrorKind::NotFound
    } else {
        FileErrorKind::Other
    }
}

fn map_err(operation: &'static str, path: &Path, e: &io::Error) -> FileError {
    FileError::new(
        operation,
        path.display().to_string(),
        kind_of(e),
        e.raw_os_error().unwrap_or(0) as u32,
        e.to_string(),
    )
}

/// Create one directory with `security`'s descriptor, or with the list it would
/// inherit when there is none. Fails with `AlreadyExists` if the name is taken,
/// which is what makes a name that wins exclusively the caller's - `std::fs::
/// create_dir` is the same call, with no way to pass a descriptor.
fn create_dir_with(path: &Path, security: Option<&DirSecurity>) -> io::Result<()> {
    let path_w = path_to_wide(path);
    let attributes = security.map(DirSecurity::attributes);
    // SAFETY: `path_w` is NUL-terminated, and the attributes, when present,
    // point at a descriptor `security` owns for longer than this call.
    let ok = unsafe {
        CreateDirectoryW(
            path_w.as_ptr(),
            attributes
                .as_ref()
                .map_or(std::ptr::null(), DirAttributes::as_ptr),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// How long a directory in the temp area must have sat untouched before a later
/// call takes it for abandoned. The window is what separates "left behind" from
/// "in use": a folder whose installer is still downloading was written to
/// minutes ago, and no caller holds one across a day.
const STALE_TEMP_DIR_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// Remove the directories earlier `create_temp_dir` calls left in `base` under
/// `prefix`. A folder outlives its process whenever something inside it is still
/// open at cleanup time - the update launches the installer out of one and the
/// OS will not delete a running image - and this is the only thing that ever
/// comes back for it.
///
/// Best effort throughout: a folder that will not go is one the next sweep tries
/// again, and an unelevated caller cannot take one an elevated run created (it
/// is that run's `release_temp_dir` that hands those to the user).
fn sweep_stale_temp_dirs(base: &Path, prefix: &str) {
    let Ok(entries) = std::fs::read_dir(base) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(prefix) {
            continue;
        }
        // A reparse point is not ours whatever it is called: following one would
        // delete what it aims at, picked by whoever could plant it here.
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_dir() || kind.is_symlink() {
            continue;
        }
        let age = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| now.duration_since(m).ok());
        if age.is_some_and(|age| age >= STALE_TEMP_DIR_AGE) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Hand what is left in `dir` to the owner of the temp area it sits in, with a
/// right to remove it and nothing else (see [`released_dir_sddl`]).
///
/// The owner is read off the enclosing area rather than taken from this
/// process's token: under over-the-shoulder elevation the token names the
/// administrator who consented at the prompt, while the area - and the leftovers
/// in it - belong to the ordinary user who asked for the update.
fn release_to_temp_area_owner(dir: &Path) -> io::Result<()> {
    let base = dir
        .parent()
        .ok_or_else(|| io::Error::other("a temp directory has an enclosing area"))?;
    set_protected_dacl(dir, &released_dir_sddl(&owner_sid_string(base)?))
}

impl Files for WindowsFiles {
    fn read(&self, path: &Path) -> Result<Vec<u8>, FileError> {
        std::fs::read(path).map_err(|e| map_err("read", path, &e))
    }

    fn write_atomic(&self, path: &Path, contents: &[u8]) -> Result<(), FileError> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| map_err("create_dir", parent, &e))?;
        }

        // Write a temp sibling, then replace the target in one atomic rename.
        // The temp name carries the process and thread ids so no two concurrent
        // writers - core threads, another core process, or the C++ app, which
        // names its temp the same way - ever share a temp file: a thread runs
        // only one write at a time, so the pair is unique across every writer
        // live at once. That leaves the rename as the only shared step, a clean
        // last-writer-wins with no torn files. std::fs::rename replaces the
        // target too, but has no way to ask for MOVEFILE_WRITE_THROUGH, so the
        // rename goes through MoveFileExW directly.
        let pid = std::process::id();
        // SAFETY: GetCurrentThreadId has no preconditions and cannot fail.
        let tid = unsafe { GetCurrentThreadId() };
        let mut temp = path.as_os_str().to_owned();
        temp.push(format!(".{pid}.{tid}.tmp"));
        let temp = PathBuf::from(temp);

        // sync_all (FlushFileBuffers) puts the contents on disk before the
        // rename publishes them, so a crash right after the rename cannot leave
        // the target present but empty or truncated.
        let written = (|| -> io::Result<()> {
            let mut file = std::fs::File::create(&temp)?;
            file.write_all(contents)?;
            file.sync_all()
        })();
        if let Err(e) = written {
            let _ = std::fs::remove_file(&temp);
            return Err(map_err("write", &temp, &e));
        }

        let temp_w = path_to_wide(&temp);
        let dest_w = path_to_wide(path);
        // SAFETY: both buffers are NUL-terminated; the flags request replacing
        // an existing target and flushing the rename itself before returning.
        let ok = unsafe {
            MoveFileExW(
                temp_w.as_ptr(),
                dest_w.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if ok == 0 {
            let os = io::Error::last_os_error();
            // Best effort cleanup of the temp file (mirrors the TS rmSync).
            let _ = std::fs::remove_file(&temp);
            return Err(FileError::new(
                "rename",
                path.display().to_string(),
                FileErrorKind::Other,
                os.raw_os_error().unwrap_or(0) as u32,
                format!("MoveFileEx: {os}"),
            ));
        }
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<DirEntry>, FileError> {
        let read_dir = std::fs::read_dir(path).map_err(|e| map_err("list", path, &e))?;
        let mut out = Vec::new();
        for entry in read_dir {
            let entry = entry.map_err(|e| map_err("list", path, &e))?;
            let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
            out.push(DirEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_file,
            });
        }
        Ok(out)
    }

    fn modified_ms(&self, path: &Path) -> Result<f64, FileError> {
        let modified = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .map_err(|e| map_err("stat", path, &e))?;
        let ms = modified
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        Ok(ms)
    }

    fn create_temp_dir(&self, prefix: &str) -> Result<PathBuf, FileError> {
        // Bound on collision retries before giving up; with nanos + a
        // process-local counter a real collision is already vanishingly rare.
        const MAX_TEMP_DIR_ATTEMPTS: usize = 64;
        let base = std::env::temp_dir();
        sweep_stale_temp_dirs(&base, prefix);
        // A unique name is not exclusive access: the temp area a privileged
        // process resolves belongs to the unelevated user, so the folder carries
        // a DACL of its own rather than the one it would inherit (see
        // `crate::security`). Built before the loop - it is the same descriptor
        // whichever name wins - and propagated rather than fallen back from,
        // since an unprotected folder is the outcome it exists to prevent.
        let security = private_dir_security().map_err(|e| {
            FileError::new(
                "create_temp_dir",
                base.display().to_string(),
                FileErrorKind::Other,
                e.raw_os_error().unwrap_or(0) as u32,
                format!("the directory's security descriptor could not be built: {e}"),
            )
        })?;
        // Try fresh names until the create succeeds (it fails if the name
        // already exists, so the winning name is exclusively ours). The name
        // mixes wall-clock nanos with a process-local counter for entropy.
        for _ in 0..MAX_TEMP_DIR_ATTEMPTS {
            let nanos = std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
            let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
            let candidate = base.join(format!("{prefix}{nanos:x}{seq:x}"));
            match create_dir_with(&candidate, security.as_ref()) {
                Ok(()) => return Ok(candidate),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(map_err("create_temp_dir", &candidate, &e)),
            }
        }
        Err(FileError::new(
            "create_temp_dir",
            base.display().to_string(),
            FileErrorKind::Other,
            0,
            "could not create a unique temp directory",
        ))
    }

    fn release_temp_dir(&self, dir: &Path) -> Result<(), FileError> {
        let Err(e) = std::fs::remove_dir_all(dir) else {
            return Ok(());
        };
        // The removal failed with the folder still there, so this is the last
        // moment anything of ours can reach it. Only a folder that got a list of
        // its own is stranded by that: an unelevated caller's carries the temp
        // area's own list, which already admits the owner, and replacing it here
        // would take rights away rather than hand them back.
        if e.kind() != io::ErrorKind::NotFound && is_elevated() {
            let _ = release_to_temp_area_owner(dir);
        }
        Err(map_err("release_temp_dir", dir, &e))
    }

    fn delete_file(&self, path: &Path) -> Result<(), FileError> {
        std::fs::remove_file(path).map_err(|e| map_err("delete", path, &e))
    }

    fn remove_dir_all(&self, path: &Path) -> Result<(), FileError> {
        std::fs::remove_dir_all(path).map_err(|e| map_err("remove_dir_all", path, &e))
    }

    fn create_dirs(&self, path: &Path) -> Result<(), FileError> {
        std::fs::create_dir_all(path).map_err(|e| map_err("create_dirs", path, &e))
    }

    fn probe_writable(&self, dir: &Path) -> Result<(), FileError> {
        // Make a write rather than read the ACL: what a write is allowed to do
        // depends on the token, on inherited denies, and on whatever filter
        // driver is in the way, and creating a file here is the same call the
        // caller is about to make. FILE_FLAG_DELETE_ON_CLOSE unlinks the probe
        // as the handle drops, including if the process dies holding it, so the
        // check leaves the folder as it found it; it needs DELETE in the access
        // mask, which GENERIC_WRITE does not carry. The name mixes the process
        // and thread ids for the reason `write_atomic`'s temp does: two probes
        // live at once must not share it.
        let pid = std::process::id();
        // SAFETY: GetCurrentThreadId has no preconditions and cannot fail.
        let tid = unsafe { GetCurrentThreadId() };
        let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let probe = dir.join(format!(".windhawk-write-probe.{pid}.{tid}.{seq:x}.tmp"));
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .access_mode(GENERIC_WRITE | DELETE)
            .custom_flags(FILE_FLAG_DELETE_ON_CLOSE)
            .open(&probe)
            // The error names the DIRECTORY: the probe file is this function's
            // own business, and the folder is what the answer is about.
            .map_err(|e| map_err("probe_writable", dir, &e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_WRITE_ATTRIBUTES,
    };

    use super::*;
    use crate::security::PRIVATE_DIR_SDDL;

    /// The descriptor is not merely well-formed, it lands on the object: a
    /// process the list does not name is refused the folder it just created,
    /// which is the guarantee read from the side it excludes. An access list
    /// that was built and then not passed to the create would show up here as a
    /// write that succeeds.
    ///
    /// Only an unelevated process can ask the question - an elevated one is
    /// admitted by the Administrators ACE, by design - so this skips otherwise
    /// and the elevated direction is covered in `tests/adapters.rs`, which reads
    /// the list back off the folder instead.
    #[test]
    fn a_folder_created_with_the_private_descriptor_refuses_everyone_it_omits() {
        if is_elevated() {
            println!("skipped: an elevated process is admitted by the list under test");
            return;
        }

        let parent = tempfile::tempdir().expect("a scratch directory");
        let dir = parent.path().join("private");
        let security = DirSecurity::from_sddl(PRIVATE_DIR_SDDL).expect("the descriptor builds");
        create_dir_with(&dir, Some(&security)).expect("the folder is created");

        let denied = std::fs::write(dir.join("planted.dll"), b"MZ")
            .expect_err("the folder must refuse a process its list omits");

        // The enclosing TempDir cannot clear this one on the way out, and says
        // nothing when it fails: the list under test grants its creator no
        // delete either, so the folder is handed back before it is removed,
        // through the WRITE_DAC its owner holds implicitly. `OW` (owner rights)
        // resolves to whoever owns the object, so naming it needs no token
        // query.
        set_protected_dacl(&dir, "D:P(A;OICI;FA;;;OW)").expect("its owner may rewrite the list");
        std::fs::remove_dir(&dir).expect("the reclaimed folder is removable");

        assert_eq!(
            denied.kind(),
            io::ErrorKind::PermissionDenied,
            "expected an access denial, got {denied}"
        );
    }

    /// The leftover an update cannot clear: the installer holds its own image
    /// open, so the folder outlives the run that made it, in a temp area whose
    /// owner the private list does not name. Released, that owner can take it
    /// away at last - and still cannot put anything in it, which is the half
    /// that keeps the release from undoing the list it widens.
    ///
    /// Unelevated, so the gate runs it: this process standing in for the
    /// ordinary user is what lets both halves be asked from the side they are
    /// meant for.
    #[test]
    fn a_released_folder_admits_the_temp_areas_owner_to_remove_it_and_nothing_more() {
        if is_elevated() {
            println!("skipped: an elevated process is admitted by the list under test");
            return;
        }

        let parent = tempfile::tempdir().expect("a scratch directory");
        let dir = parent.path().join("windhawk_update_leftover");
        std::fs::create_dir(&dir).expect("the folder is created");
        std::fs::write(dir.join("windhawk_setup.exe"), b"MZ").expect("the installer is written");
        // The list the folder would carry had this process been elevated,
        // reaching the file already in it the way inheritance reaches the one
        // the download writes afterwards.
        set_protected_dacl(&dir, PRIVATE_DIR_SDDL).expect("its owner may rewrite the list");
        std::fs::remove_dir_all(&dir).expect_err("the private list must refuse the removal");

        release_to_temp_area_owner(&dir).expect("the folder is released to the area's owner");

        std::fs::write(dir.join("planted.dll"), b"MZ")
            .expect_err("a released folder must still refuse what it is protecting against");
        std::fs::remove_dir_all(&dir).expect("the released folder is removable");
    }

    /// A folder left in the temp area is swept by the next call under the same
    /// prefix, once nothing could still be using it. The age is the whole of the
    /// distinction, so both sides of it are asserted: a fresh folder is exactly
    /// what a concurrent caller's looks like.
    #[test]
    fn creating_a_temp_dir_sweeps_the_stale_folders_of_earlier_calls() {
        let base = tempfile::tempdir().expect("a scratch directory");

        let stale = base.path().join("prefix_stale");
        std::fs::create_dir(&stale).expect("the stale folder is created");
        std::fs::write(stale.join("windhawk_setup.exe"), b"MZ").expect("its leftover is written");
        backdate(&stale, STALE_TEMP_DIR_AGE + Duration::from_secs(60));

        let fresh = base.path().join("prefix_fresh");
        std::fs::create_dir(&fresh).expect("the fresh folder is created");

        let other = base.path().join("unrelated");
        std::fs::create_dir(&other).expect("the unrelated folder is created");
        backdate(&other, STALE_TEMP_DIR_AGE + Duration::from_secs(60));

        sweep_stale_temp_dirs(base.path(), "prefix_");

        assert!(!stale.exists(), "a stale folder and its contents are swept");
        assert!(fresh.exists(), "a folder a live caller could hold is kept");
        assert!(
            other.exists(),
            "another prefix's folder is not ours to sweep"
        );
    }

    /// Move a directory's last-write time `age` into the past, so the sweep sees
    /// it as one nothing is holding. Opening a directory at all needs
    /// FILE_FLAG_BACKUP_SEMANTICS.
    fn backdate(dir: &Path, age: Duration) {
        std::fs::OpenOptions::new()
            .access_mode(FILE_WRITE_ATTRIBUTES)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(dir)
            .expect("the folder opens for a time change")
            .set_modified(SystemTime::now() - age)
            .expect("its last-write time is set");
    }
}
