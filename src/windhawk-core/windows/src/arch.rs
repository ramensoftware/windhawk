//! ARM64 native-machine detection: whether Windows is running on ARM64
//! hardware, which gates the extra aarch64 compile target. `IsWow64Process2`
//! reports the OS native machine even from an x86/x64 (WOW64) process, so a
//! 32-bit or 64-bit Windhawk on an ARM64 OS still detects it. The export is
//! resolved dynamically from kernel32 so the module still loads on OSes that
//! predate `IsWow64Process2` (Windows before 10 1709), where its absence reads
//! as non-ARM64.
//!
//! This is where the arm64 flag is detected for the whole workspace: the core
//! resolves it here (through the composition root) at session creation rather
//! than having it handed in through the process environment.

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::SystemInformation::IMAGE_FILE_MACHINE_ARM64;
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use crate::wide::to_wide;

/// Whether the OS native machine is ARM64.
pub fn is_arm64_native_machine() -> bool {
    // The third and fourth params are IMAGE_FILE_MACHINE out-values (`u16`); the
    // return is a Win32 BOOL (`i32`). IsWow64Process2 is resolved dynamically
    // (below), so the signature is declared here rather than imported.
    type IsWow64Process2Fn = unsafe extern "system" fn(HANDLE, *mut u16, *mut u16) -> i32;

    let kernel32 = to_wide("kernel32.dll");
    // SAFETY: kernel32.dll is always loaded; GetModuleHandleW takes a
    // NUL-terminated wide string (to_wide guarantees the NUL) and returns its
    // module handle or null, which is checked before use.
    let module = unsafe { GetModuleHandleW(kernel32.as_ptr()) };
    if module.is_null() {
        return false;
    }

    // SAFETY: `module` is kernel32's handle; the proc name is a NUL-terminated C
    // string. GetProcAddress returns None when the export is absent (an OS
    // predating IsWow64Process2), handled below.
    let Some(proc) = (unsafe { GetProcAddress(module, c"IsWow64Process2".as_ptr().cast()) }) else {
        // ARM64 OSes always export IsWow64Process2; its absence means a pre-1709
        // OS, which is not ARM64.
        return false;
    };

    // SAFETY: IsWow64Process2 has exactly this signature; transmuting the
    // resolved export to it is the documented dynamic-call pattern.
    let is_wow64_process2 = unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, IsWow64Process2Fn>(proc)
    };

    let mut process_machine: u16 = 0;
    let mut native_machine: u16 = 0;
    // SAFETY: GetCurrentProcess returns the current-process pseudo-handle; the
    // two out-params point at valid locals for the duration of the call.
    let ok = unsafe {
        is_wow64_process2(
            GetCurrentProcess(),
            &mut process_machine,
            &mut native_machine,
        )
    };
    ok != 0 && native_machine == IMAGE_FILE_MACHINE_ARM64
}
