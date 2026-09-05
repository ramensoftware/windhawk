//! Keeps a wedged Explorer from pinning the launch.
//!
//! tao creates every window with an unconditional
//! `ITaskbarList::AddTab(hwnd)` - it routes the `skip_taskbar` builder attribute
//! through one call that does `DeleteTab` when set and `AddTab` when not, and
//! makes that call whether or not the attribute was ever asked for
//! (`platform_impl/windows/window.rs`, in the tail of `init`). `AddTab` on a
//! plain top-level window is a no-op: such a window is in the taskbar already,
//! and `AddTab` is only meaningful as the undo of a previous `DeleteTab`. What
//! it costs is not nothing, though. explorerframe.dll implements it as a
//! `SendMessage` to the taskbar's window, which is owned by Explorer - and
//! `SendMessage` has no timeout. An Explorer that has stopped pumping therefore
//! parks the thread inside the window build, forever, before the app has drawn
//! anything. The startup watchdog catches the symptom, and a relaunch walks
//! straight back into it.
//!
//! So the class is taken away from tao for exactly the spans in which it makes
//! that call. `CoRegisterClassObject` puts a class object in the process's own
//! class table, which `CoGetClassObject` consults ahead of the registry for an
//! in-process context, so tao's
//! `CoCreateInstance(CLSID_TaskbarList, .., CLSCTX_SERVER)` reaches [`STUB`]
//! instead of explorerframe's. The stub answers every `ITaskbarList` method with
//! `S_OK` and does nothing, which is what `AddTab` would have achieved on that
//! window anyway, minus the trip to Explorer.
//!
//! Handing back an object rather than failing the activation is deliberate:
//! tao discards the error from the call this exists to defuse (`let _ = ..`),
//! but two of its siblings (`set_progress_bar`, `set_overlay_icon`) `unwrap()`
//! the same `CoCreateInstance`, so a factory that refuses to create would turn a
//! future call into a panic instead of a hang.
//!
//! There are two such spans, and the second one hangs a healthy machine.
//! [`guard_taskbar_restart`] covers it: the shell broadcasts `TaskbarCreated`
//! whenever the taskbar is recreated, and tao answers that broadcast from the
//! window procedure by taking the window-state lock and calling the same
//! `AddTab` while the guard is still alive. That `SendMessage` is delivered on
//! this very thread, so it re-enters the window procedure, whose
//! `WM_ERASEBKGND` arm reaches for the same non-reentrant lock - and the UI
//! thread parks forever on a lock it holds itself. Explorer does not have to be
//! wedged for this one; the broadcast is enough.
//!
//! The registration is a [`Suppressed`] guard, not a process-wide switch: each
//! span revokes it on the way out, so every other `CLSID_TaskbarList` in this
//! process - tao's own, WebView2's, anything a dependency grows - gets the real
//! Explorer object. The only calls it is allowed to swallow are the ones made
//! while it is held. That matters beyond tidiness: the factory answers
//! `E_NOINTERFACE` for `ITaskbarList3`, so a registration left standing would
//! turn someone else's progress or overlay call into a failure.
//!
//! Remove [`guard_taskbar_restart`] once `tauri-runtime-wry` picks up tao 0.36,
//! which releases the window-state lock before updating taskbar visibility.
//! Remove [`suppress`] with it once tao also stops calling `AddTab` for a window
//! that never asked to skip the taskbar, which is what the build span is for.

use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{E_NOINTERFACE, HWND, LPARAM, LRESULT, S_OK, WPARAM};
use windows_sys::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoInitializeEx, CoRegisterClassObject,
    CoRevokeClassObject, REGCLS_MULTIPLEUSE,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW;
use windows_sys::core::{GUID, HRESULT};

/// `CLSID_TaskbarList`, the class tao activates.
const CLSID_TASKBAR_LIST: GUID = GUID::from_u128(0x56fdf344_fd6d_11d0_958a_006097c9a090);
/// `IID_ITaskbarList`, the only interface the stub serves.
const IID_ITASKBARLIST: GUID = GUID::from_u128(0x56fdf342_fd6d_11d0_958a_006097c9a090);
const IID_ICLASSFACTORY: GUID = GUID::from_u128(0x00000001_0000_0000_c000_000000000046);
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);

/// The class factory's answer to an aggregation request, which the stub does not
/// support (and which nothing asks it for).
const CLASS_E_NOAGGREGATION: HRESULT = 0x8004_0110u32 as HRESULT;

/// `GUID` is a plain `#[repr(C)]` record in windows-sys with no `PartialEq`.
fn guid_eq(a: &GUID, b: &GUID) -> bool {
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}

/// The live registration. Dropping it revokes the class object, putting the real
/// `CLSID_TaskbarList` back for the rest of the process.
///
/// A registration that never happened is still a valid guard: the caller wants
/// the window either way, and a failure here only means the build takes its
/// chances with Explorer, exactly as it did before.
pub struct Suppressed {
    cookie: u32,
}

impl Drop for Suppressed {
    fn drop(&mut self) {
        if self.cookie != 0 {
            // SAFETY: `cookie` is the registration `CoRegisterClassObject`
            // handed back below, revoked once because a `Suppressed` is neither
            // `Clone` nor `Copy`.
            unsafe { CoRevokeClassObject(self.cookie) };
        }
    }
}

/// Take `CLSID_TaskbarList` over for the lifetime of the returned guard.
///
/// Hold it across a tao window build and nothing else: the point is to be the
/// narrowest possible window in which this process answers for a shell class.
pub fn suppress() -> Suppressed {
    // The class table is per-apartment, so the caller's thread has to be in one.
    // Whichever answer this gets - the apartment is ours, someone got here first
    // (`S_FALSE`), or the thread is already in an MTA (`RPC_E_CHANGED_MODE`) -
    // COM is up on this thread afterwards and tao will activate in the same
    // apartment it registers in. Deliberately not balanced by `CoUninitialize`:
    // the thread that builds the window keeps its apartment for the life of the
    // process, and tao holds the same view (its `com_initialized` releases only
    // at thread exit).
    //
    // SAFETY: no reserved parameter, and a documented concurrency constant.
    unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };

    let mut cookie = 0u32;
    // SAFETY: the CLSID and the cookie are ours; the class object is a `'static`
    // whose vtable pointer is fixed at compile time, so it outlives the
    // registration and cannot move under it.
    let hr = unsafe {
        CoRegisterClassObject(
            &CLSID_TASKBAR_LIST,
            std::ptr::addr_of!(FACTORY.0) as *mut c_void,
            CLSCTX_INPROC_SERVER,
            REGCLS_MULTIPLEUSE as u32,
            &mut cookie,
        )
    };

    Suppressed {
        cookie: if hr == S_OK { cookie } else { 0 },
    }
}

/// Put the stub back for the shell's `TaskbarCreated` broadcast, on the window
/// tao answers it for.
///
/// Must be called on the thread that owns `hwnd`, and once the window is built:
/// `SetWindowSubclass` runs the most recently installed procedure first, so a
/// subclass added after tao's sees the broadcast ahead of it.
///
/// Best effort: a subclass the system declines leaves the broadcast reaching tao
/// with the real class in place, which is the deadlock this defuses - there is
/// nothing further to fall back to, and refusing to open the window over it
/// would trade a hang that needs a taskbar restart for one that needs nothing.
pub fn guard_taskbar_restart(hwnd: HWND) {
    // SAFETY: `hwnd` is the live main window and this runs on the thread that
    // owns it, as subclassing requires. The procedure is a plain fn of the
    // documented signature, registered under an id this module owns, and takes
    // no reference data to outlive.
    unsafe {
        SetWindowSubclass(hwnd, Some(taskbar_restart_subclass), RESTART_SUBCLASS_ID, 0);
    }
}

/// The id [`guard_taskbar_restart`] registers its subclass under, which only has
/// to be unique among the subclasses this module puts on a window.
const RESTART_SUBCLASS_ID: usize = 1;

/// Hold the stub over tao's handling of `TaskbarCreated`, and over nothing else.
///
/// The guard has to span the rest of the chain rather than wrap a call of our
/// own, because the `AddTab` being defused is tao's, further down it.
unsafe extern "system" fn taskbar_restart_subclass(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _ref_data: usize,
) -> LRESULT {
    let restart = taskbar_created_message();
    // Bound rather than dropped: this keeps the class ours until the procedure
    // returns, which is what puts the stub in front of the `AddTab` below.
    let _suppressed = (restart != 0 && msg == restart).then(suppress);

    // SAFETY: the arguments are the ones this procedure was handed, passed on to
    // the rest of the subclass chain as the contract requires.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// `TaskbarCreated`, the broadcast the shell makes when the taskbar is recreated,
/// under the name tao registers it by.
///
/// Registered once and remembered, since the id is fixed for the life of the
/// session. Zero is the failure answer, and is what the caller tests for rather
/// than compares against: message zero is `WM_NULL`, which arrives on its own and
/// is nothing to hold a shell class over.
fn taskbar_created_message() -> u32 {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    *MESSAGE.get_or_init(|| {
        let name: Vec<u16> = "TaskbarCreated"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: `name` is a null-terminated UTF-16 string that outlives the call.
        unsafe { RegisterWindowMessageW(name.as_ptr()) }
    })
}

// The two COM objects below are `'static` singletons with no state: their
// `AddRef`/`Release` return plausible non-zero counts rather than tracking one,
// because there is nothing to free and no moment at which they should stop
// answering. Both are reachable only through the registration above, so the
// lifetime that matters is the guard's, not a refcount's.

#[repr(C)]
struct ClassFactoryVtbl {
    query_interface:
        unsafe extern "system" fn(*mut ClassFactory, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut ClassFactory) -> u32,
    release: unsafe extern "system" fn(*mut ClassFactory) -> u32,
    create_instance: unsafe extern "system" fn(
        *mut ClassFactory,
        *mut c_void,
        *const GUID,
        *mut *mut c_void,
    ) -> HRESULT,
    lock_server: unsafe extern "system" fn(*mut ClassFactory, i32) -> HRESULT,
}

#[repr(C)]
struct ClassFactory {
    vtbl: *const ClassFactoryVtbl,
}

unsafe extern "system" fn factory_query_interface(
    this: *mut ClassFactory,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: COM passes a readable IID and a writable slot.
    unsafe {
        if guid_eq(&*iid, &IID_IUNKNOWN) || guid_eq(&*iid, &IID_ICLASSFACTORY) {
            *out = this.cast();
            S_OK
        } else {
            *out = null_mut();
            E_NOINTERFACE
        }
    }
}

unsafe extern "system" fn factory_add_ref(_this: *mut ClassFactory) -> u32 {
    2
}

unsafe extern "system" fn factory_release(_this: *mut ClassFactory) -> u32 {
    1
}

unsafe extern "system" fn factory_create_instance(
    _this: *mut ClassFactory,
    outer: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: COM passes a readable IID and a writable slot.
    unsafe {
        *out = null_mut();
        if !outer.is_null() {
            return CLASS_E_NOAGGREGATION;
        }
        if !guid_eq(&*iid, &IID_IUNKNOWN) && !guid_eq(&*iid, &IID_ITASKBARLIST) {
            // Anything beyond plain `ITaskbarList` - the progress and overlay
            // work of `ITaskbarList3`, say - is not what this stands in for, and
            // saying so sends the caller to the real class rather than to a
            // silence it did not ask for.
            return E_NOINTERFACE;
        }
        *out = std::ptr::addr_of!(STUB.0) as *mut c_void;
        S_OK
    }
}

unsafe extern "system" fn factory_lock_server(_this: *mut ClassFactory, _lock: i32) -> HRESULT {
    S_OK
}

static FACTORY_VTBL: ClassFactoryVtbl = ClassFactoryVtbl {
    query_interface: factory_query_interface,
    add_ref: factory_add_ref,
    release: factory_release,
    create_instance: factory_create_instance,
    lock_server: factory_lock_server,
};

/// The `ITaskbarList` vtable: `IUnknown`, then the five methods of the interface
/// in declaration order.
#[repr(C)]
struct TaskbarListVtbl {
    query_interface:
        unsafe extern "system" fn(*mut TaskbarList, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut TaskbarList) -> u32,
    release: unsafe extern "system" fn(*mut TaskbarList) -> u32,
    hr_init: unsafe extern "system" fn(*mut TaskbarList) -> HRESULT,
    add_tab: unsafe extern "system" fn(*mut TaskbarList, HWND) -> HRESULT,
    delete_tab: unsafe extern "system" fn(*mut TaskbarList, HWND) -> HRESULT,
    activate_tab: unsafe extern "system" fn(*mut TaskbarList, HWND) -> HRESULT,
    set_active_alt: unsafe extern "system" fn(*mut TaskbarList, HWND) -> HRESULT,
}

#[repr(C)]
struct TaskbarList {
    vtbl: *const TaskbarListVtbl,
}

unsafe extern "system" fn stub_query_interface(
    this: *mut TaskbarList,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: COM passes a readable IID and a writable slot.
    unsafe {
        if guid_eq(&*iid, &IID_IUNKNOWN) || guid_eq(&*iid, &IID_ITASKBARLIST) {
            *out = this.cast();
            S_OK
        } else {
            *out = null_mut();
            E_NOINTERFACE
        }
    }
}

unsafe extern "system" fn stub_add_ref(_this: *mut TaskbarList) -> u32 {
    2
}

unsafe extern "system" fn stub_release(_this: *mut TaskbarList) -> u32 {
    1
}

/// Every method: report success, touch nothing. The taskbar decides for itself
/// which top-level windows get a button, and this window is one of them.
unsafe extern "system" fn stub_ok(_this: *mut TaskbarList) -> HRESULT {
    S_OK
}

unsafe extern "system" fn stub_ok_hwnd(_this: *mut TaskbarList, _hwnd: HWND) -> HRESULT {
    S_OK
}

static STUB_VTBL: TaskbarListVtbl = TaskbarListVtbl {
    query_interface: stub_query_interface,
    add_ref: stub_add_ref,
    release: stub_release,
    hr_init: stub_ok,
    add_tab: stub_ok_hwnd,
    delete_tab: stub_ok_hwnd,
    activate_tab: stub_ok_hwnd,
    set_active_alt: stub_ok_hwnd,
};

/// A `static` holding a raw vtable pointer is not `Sync` on its own. Both
/// objects are immutable for the life of the process and their methods keep no
/// state, so sharing them across threads is sound.
struct Shared<T>(T);
// SAFETY: `T` here is a bare vtable pointer to a `'static`, never written after
// initialization; the methods it names read no shared mutable state.
unsafe impl<T> Sync for Shared<T> {}

static FACTORY: Shared<ClassFactory> = Shared(ClassFactory {
    vtbl: &FACTORY_VTBL,
});
static STUB: Shared<TaskbarList> = Shared(TaskbarList { vtbl: &STUB_VTBL });

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Com::CLSCTX_SERVER;

    unsafe fn co_create_taskbar_list() -> (HRESULT, *mut c_void) {
        let mut ppv: *mut c_void = null_mut();
        // SAFETY: the CLSID and IID are consts, `ppv` is a live local, and the
        // only caller has initialized COM on this thread.
        let hr = unsafe {
            windows_sys::Win32::System::Com::CoCreateInstance(
                &CLSID_TASKBAR_LIST,
                null_mut(),
                CLSCTX_SERVER,
                &IID_ITASKBARLIST,
                &mut ppv,
            )
        };
        (hr, ppv)
    }

    /// Is this the stub, or Explorer's? The stub is the one `static`, so its
    /// identity is its address - no call required, and no dependence on what the
    /// real object would have answered.
    fn is_stub(ptr: *mut c_void) -> bool {
        std::ptr::eq(ptr.cast_const(), std::ptr::addr_of!(STUB.0).cast())
    }

    /// The whole contract in one thread, because the class table this registers
    /// in is per-apartment: while the guard is held tao's activation lands on the
    /// stub, and once it is dropped the class belongs to Explorer again.
    ///
    /// Only what is asked for is asserted. Whether `CLSID_TaskbarList` resolves
    /// at all is Explorer's business and not this test's - a session without a
    /// shell is a legitimate environment - so the unsuppressed shots assert only
    /// that they are *not* the stub.
    #[test]
    fn suppresses_taskbar_list_for_the_life_of_the_guard() {
        // SAFETY: single-threaded test; every pointer below is released before
        // it goes out of scope.
        unsafe {
            CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32);

            let (_, before) = co_create_taskbar_list();
            assert!(
                !is_stub(before),
                "the stub answered before it was registered"
            );
            release(before);

            let guard = suppress();
            assert_ne!(guard.cookie, 0, "the class object was not registered");

            let (hr, during) = co_create_taskbar_list();
            assert_eq!(hr, S_OK, "the stub refused to activate");
            assert!(is_stub(during), "the activation missed the stub");
            // The methods tao reaches for, all no-ops that report success.
            assert_eq!((STUB_VTBL.hr_init)(during.cast()), S_OK);
            assert_eq!((STUB_VTBL.add_tab)(during.cast(), null_mut()), S_OK);
            assert_eq!((STUB_VTBL.delete_tab)(during.cast(), null_mut()), S_OK);
            release(during);

            drop(guard);

            let (_, after) = co_create_taskbar_list();
            assert!(!is_stub(after), "the stub outlived its guard");
            release(after);
        }
    }

    /// The broadcast resolves to a registered message, which is what makes the
    /// subclass's comparison mean anything: a registration that failed answers
    /// zero, and zero is a message that arrives on its own.
    #[test]
    fn taskbar_created_is_a_registered_message() {
        let message = taskbar_created_message();
        assert!(
            (0xC000..=0xFFFF).contains(&message),
            "TaskbarCreated resolved to {message:#x}, outside the registered-message range"
        );
        assert_eq!(
            message,
            taskbar_created_message(),
            "the id changed between calls"
        );
    }

    unsafe fn release(ptr: *mut c_void) {
        if ptr.is_null() {
            return;
        }
        // SAFETY: `IUnknown::Release` is vtable slot 2 for every COM object.
        unsafe {
            let vtbl = *ptr.cast::<*const unsafe extern "system" fn(*mut c_void) -> u32>();
            (*vtbl.add(2))(ptr);
        }
    }
}
