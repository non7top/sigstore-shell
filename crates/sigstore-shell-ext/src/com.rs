use crate::dll::{guard, lock_server, Live};
use crate::page;
use std::cell::RefCell;
use std::ffi::c_void;
use std::path::PathBuf;
use windows::core::{implement, Interface, Ref, Result, BOOL, GUID, HRESULT};
use windows::Win32::Foundation::{
    CLASS_E_NOAGGREGATION, E_FAIL, E_INVALIDARG, E_NOTIMPL, E_POINTER, LPARAM,
};
use windows::Win32::System::Com::DVASPECT_CONTENT;
use windows::Win32::System::Com::{
    IClassFactory, IClassFactory_Impl, IDataObject, FORMATETC, TYMED_HGLOBAL,
};
use windows::Win32::System::Ole::{ReleaseStgMedium, CF_HDROP};
use windows::Win32::System::Registry::HKEY;
use windows::Win32::UI::Controls::{DestroyPropertySheetPage, LPFNSVADDPROPSHEETPAGE};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    DragQueryFileW, IShellExtInit, IShellExtInit_Impl, IShellPropSheetExt, IShellPropSheetExt_Impl,
    HDROP,
};

#[implement(IClassFactory)]
pub struct Factory {
    _live: Live,
}

impl Factory {
    pub fn new() -> Self {
        Self { _live: Live::new() }
    }
}

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<'_, windows::core::IUnknown>,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> Result<()> {
        if ppv.is_null() || riid.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *ppv = std::ptr::null_mut() };
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let obj: windows::core::IUnknown = SigstoreExt::new().into();
        unsafe { obj.query(riid, ppv).ok() }
    }

    fn LockServer(&self, lock: BOOL) -> Result<()> {
        lock_server(lock.as_bool());
        Ok(())
    }
}

#[implement(IShellExtInit, IShellPropSheetExt)]
pub struct SigstoreExt {
    file: RefCell<Option<PathBuf>>,
    _live: Live,
}

impl SigstoreExt {
    fn new() -> Self {
        Self {
            file: RefCell::new(None),
            _live: Live::new(),
        }
    }
}

/// The one selected `.exe`, or `None` for a multi-selection or another file type.
unsafe fn selected_exe(data: &IDataObject) -> Option<PathBuf> {
    let format = FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    let mut medium = data.GetData(&format).ok()?;
    let hdrop = HDROP(medium.u.hGlobal.0);
    let count = DragQueryFileW(hdrop, u32::MAX, None);
    let path = (count == 1).then(|| {
        let len = DragQueryFileW(hdrop, 0, None) as usize;
        let mut buf = vec![0u16; len + 1];
        let n = DragQueryFileW(hdrop, 0, Some(&mut buf)) as usize;
        PathBuf::from(String::from_utf16_lossy(&buf[..n]))
    });
    ReleaseStgMedium(&mut medium);
    path.filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")))
}

impl IShellExtInit_Impl for SigstoreExt_Impl {
    fn Initialize(
        &self,
        _folder: *const ITEMIDLIST,
        data: Ref<'_, IDataObject>,
        _prog_id: HKEY,
    ) -> Result<()> {
        let hr = guard(|| {
            let Some(data) = data.as_ref() else {
                return E_INVALIDARG;
            };
            match unsafe { selected_exe(data) } {
                Some(path) => {
                    *self.file.borrow_mut() = Some(path);
                    HRESULT(0)
                }
                None => E_INVALIDARG,
            }
        });
        hr.ok()
    }
}

impl IShellPropSheetExt_Impl for SigstoreExt_Impl {
    fn AddPages(&self, add: LPFNSVADDPROPSHEETPAGE, lparam: LPARAM) -> Result<()> {
        let hr = guard(|| {
            let (Some(add), Some(path)) = (add, self.file.borrow().clone()) else {
                return E_FAIL;
            };
            let Ok(page) = page::create(path) else {
                return E_FAIL;
            };
            if unsafe { add(page, lparam) }.as_bool() {
                HRESULT(0)
            } else {
                let _ = unsafe { DestroyPropertySheetPage(page) };
                E_FAIL
            }
        });
        hr.ok()
    }

    fn ReplacePage(
        &self,
        _page_id: u32,
        _replace: LPFNSVADDPROPSHEETPAGE,
        _lparam: LPARAM,
    ) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
}
