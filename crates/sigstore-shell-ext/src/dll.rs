use crate::com::Factory;
use crate::registration::CLSID_U128;
use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicI32, AtomicIsize, Ordering};
use windows::core::{Interface, BOOL, GUID, HRESULT};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, E_FAIL, E_POINTER, HINSTANCE, HMODULE, S_FALSE, S_OK, TRUE,
};
use windows::Win32::System::Com::IClassFactory;
use windows::Win32::System::LibraryLoader::DisableThreadLibraryCalls;
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;

static MODULE: AtomicIsize = AtomicIsize::new(0);
/// Live factories, extension objects, property pages and server locks.
static LIVE: AtomicI32 = AtomicI32::new(0);

pub const CLSID_SIGSTORE: GUID = GUID::from_u128(CLSID_U128);

pub fn module() -> HMODULE {
    HMODULE(MODULE.load(Ordering::Relaxed) as *mut c_void)
}

/// Keeps the DLL from being unloaded while something it handed out is alive.
pub struct Live;

impl Live {
    pub fn new() -> Self {
        LIVE.fetch_add(1, Ordering::SeqCst);
        Live
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn lock_server(lock: bool) {
    LIVE.fetch_add(if lock { 1 } else { -1 }, Ordering::SeqCst);
}

/// A panic must not unwind into Explorer.
pub fn guard(f: impl FnOnce() -> HRESULT) -> HRESULT {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(E_FAIL)
}

pub fn guard_value<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(fallback)
}

#[no_mangle]
extern "system" fn DllMain(hinst: HINSTANCE, reason: u32, _reserved: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        MODULE.store(hinst.0 as isize, Ordering::Relaxed);
        // Failure only means thread notifications keep arriving, which the DLL ignores.
        let _ = unsafe { DisableThreadLibraryCalls(HMODULE(hinst.0)) };
    }
    TRUE
}

#[no_mangle]
unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    guard(|| {
        if rclsid.is_null() || riid.is_null() || ppv.is_null() {
            return E_POINTER;
        }
        *ppv = std::ptr::null_mut();
        if *rclsid != CLSID_SIGSTORE {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
        let factory: IClassFactory = Factory::new().into();
        factory.query(riid, ppv)
    })
}

#[no_mangle]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    if LIVE.load(Ordering::SeqCst) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

#[no_mangle]
extern "system" fn DllRegisterServer() -> HRESULT {
    guard(crate::registry::register)
}

#[no_mangle]
extern "system" fn DllUnregisterServer() -> HRESULT {
    guard(crate::registry::unregister)
}
