use crate::dll::module;
use crate::registration::{approved_key, keys_to_delete, values, CLSID};
use windows::core::{HRESULT, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, MAX_PATH, WIN32_ERROR};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegDeleteTreeW, RegSetValueExW, HKEY,
    HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};

const SELFREG_E_CLASS: HRESULT = HRESULT(0x8004_0201_u32 as i32);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn check(err: WIN32_ERROR) -> Result<(), HRESULT> {
    if err == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(HRESULT::from_win32(err.0))
    }
}

fn dll_path() -> Result<String, HRESULT> {
    let mut buf = [0u16; MAX_PATH as usize * 2];
    let n = unsafe { GetModuleFileNameW(Some(module()), &mut buf) } as usize;
    if n == 0 || n >= buf.len() {
        return Err(SELFREG_E_CLASS);
    }
    Ok(String::from_utf16_lossy(&buf[..n]))
}

fn set_value(key: &str, name: Option<&str>, data: &str) -> Result<(), HRESULT> {
    let key = wide(key);
    let name = name.map(wide);
    let data: Vec<u8> = wide(data).into_iter().flat_map(u16::to_le_bytes).collect();
    let mut hkey = HKEY::default();
    unsafe {
        check(RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE | KEY_WOW64_64KEY,
            None,
            &mut hkey,
            None,
        ))?;
        let name_ptr = name.as_ref().map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr()));
        let result = check(RegSetValueExW(hkey, name_ptr, None, REG_SZ, Some(&data)));
        let _ = RegCloseKey(hkey);
        result
    }
}

fn collapse(result: Result<(), HRESULT>) -> HRESULT {
    match result {
        Ok(()) => HRESULT(0),
        Err(e) => e,
    }
}

fn notify_shell() {
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
}

pub fn register() -> HRESULT {
    let result = (|| {
        let path = dll_path()?;
        for v in values(&path) {
            set_value(&v.key, v.name, &v.data)?;
        }
        Ok(())
    })();
    notify_shell();
    collapse(result)
}

pub fn unregister() -> HRESULT {
    let result = (|| {
        for key in keys_to_delete() {
            let key = wide(&key);
            let err = unsafe { RegDeleteTreeW(HKEY_LOCAL_MACHINE, PCWSTR(key.as_ptr())) };
            if err != ERROR_FILE_NOT_FOUND {
                check(err)?;
            }
        }
        let (approved, clsid) = (wide(approved_key()), wide(CLSID));
        let err = unsafe {
            RegDeleteKeyValueW(
                HKEY_LOCAL_MACHINE,
                PCWSTR(approved.as_ptr()),
                PCWSTR(clsid.as_ptr()),
            )
        };
        if err != ERROR_FILE_NOT_FOUND {
            check(err)?;
        }
        Ok(())
    })();
    notify_shell();
    collapse(result)
}
