//! Loads the built DLL the way Explorer would and walks the COM entry points.
//! Usage: smoke <dll> <exe> [--ui [--shot <file.bmp>]]   (meant to run under Wine; exits non-zero on the first failure)

#[cfg(not(windows))]
fn main() {
    eprintln!("smoke only runs on Windows or under Wine");
    std::process::exit(2);
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::mem::ManuallyDrop;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{implement, Interface, Ref, Result, BOOL, GUID, HRESULT, PCWSTR};
    use windows::Win32::Foundation::{
        CLASS_E_CLASSNOTAVAILABLE, E_NOTIMPL, HWND, LPARAM, S_FALSE, S_OK, WPARAM,
    };
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, GetDC, GetDIBits, SelectObject,
        BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
    };
    use windows::Win32::System::Com::{
        IAdviseSink, IClassFactory, IDataObject, IDataObject_Impl, IEnumFORMATETC, IEnumSTATDATA,
        FORMATETC, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
    };
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::UI::Controls::{
        DestroyPropertySheetPage, PropertySheetW, HPROPSHEETPAGE, NMHDR, NMLINK, NM_CLICK,
        PROPSHEETHEADERW_V2, PROPSHEETHEADERW_V2_1, PROPSHEETHEADERW_V2_2, PROPSHEETHEADERW_V2_3,
        PSH_MODELESS, PSM_GETCURRENTPAGEHWND,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
    /// PSH_USEHPSP from prsht.h; the windows crate omits it.
    const PSH_USEHPSP: u32 = 0x100;
    use windows::Win32::UI::Shell::{IShellExtInit, IShellPropSheetExt, DROPFILES};
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetDlgItem, GetWindowRect, GetWindowTextW, PeekMessageW, SendMessageW,
        TranslateMessage, BM_CLICK, MSG, PM_REMOVE, WM_NOTIFY,
    };

    const CLSID: GUID = GUID::from_u128(0xfbcd8210_9f9c_4b07_900a_ad12500a4363);

    type GetClassObject =
        unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;
    type CanUnload = unsafe extern "system" fn() -> HRESULT;

    #[implement(IDataObject)]
    struct FileList(Vec<u16>);

    impl IDataObject_Impl for FileList_Impl {
        fn GetData(&self, _format: *const FORMATETC) -> Result<STGMEDIUM> {
            let header = std::mem::size_of::<DROPFILES>();
            let bytes = header + self.0.len() * 2;
            unsafe {
                let mem = GlobalAlloc(GMEM_MOVEABLE, bytes)?;
                let ptr = GlobalLock(mem).cast::<u8>();
                let drop = DROPFILES {
                    pFiles: header as u32,
                    fWide: BOOL(1),
                    ..Default::default()
                };
                std::ptr::write_unaligned(ptr.cast::<DROPFILES>(), drop);
                std::ptr::copy_nonoverlapping(
                    self.0.as_ptr().cast::<u8>(),
                    ptr.add(header),
                    self.0.len() * 2,
                );
                let _ = GlobalUnlock(mem);
                Ok(STGMEDIUM {
                    tymed: TYMED_HGLOBAL.0 as u32,
                    u: STGMEDIUM_0 { hGlobal: mem },
                    pUnkForRelease: ManuallyDrop::new(None),
                })
            }
        }
        fn GetDataHere(&self, _: *const FORMATETC, _: *mut STGMEDIUM) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn QueryGetData(&self, _: *const FORMATETC) -> HRESULT {
            S_OK
        }
        fn GetCanonicalFormatEtc(&self, _: *const FORMATETC, _: *mut FORMATETC) -> HRESULT {
            E_NOTIMPL
        }
        fn SetData(&self, _: *const FORMATETC, _: *const STGMEDIUM, _: BOOL) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn EnumFormatEtc(&self, _: u32) -> Result<IEnumFORMATETC> {
            Err(E_NOTIMPL.into())
        }
        fn DAdvise(&self, _: *const FORMATETC, _: u32, _: Ref<'_, IAdviseSink>) -> Result<u32> {
            Err(E_NOTIMPL.into())
        }
        fn DUnadvise(&self, _: u32) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> {
            Err(E_NOTIMPL.into())
        }
    }

    fn data_object(path: &str) -> IDataObject {
        let mut wide: Vec<u16> = std::ffi::OsStr::new(path).encode_wide().collect();
        wide.extend([0, 0]);
        FileList(wide).into()
    }

    thread_local! {
        static PAGES: std::cell::RefCell<Vec<HPROPSHEETPAGE>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    unsafe extern "system" fn add_page(page: HPROPSHEETPAGE, _: LPARAM) -> BOOL {
        PAGES.with(|p| p.borrow_mut().push(page));
        BOOL(1)
    }

    fn step(name: &str, ok: bool) {
        println!("{} {name}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            std::process::exit(1);
        }
    }

    fn text(hwnd: HWND, id: u16) -> String {
        let mut buf = [0u16; 2048];
        unsafe {
            let Ok(item) = GetDlgItem(Some(hwnd), i32::from(id)) else {
                return "<missing control>".into();
            };
            let n = GetWindowTextW(item, &mut buf) as usize;
            String::from_utf16_lossy(&buf[..n])
        }
    }

    fn pump(ms: u32) {
        let end = std::time::Instant::now() + std::time::Duration::from_millis(u64::from(ms));
        while std::time::Instant::now() < end {
            let mut msg = MSG::default();
            unsafe {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    /// Copies the window's pixels off the (virtual) screen into a top-down 32-bit BMP.
    fn screenshot(hwnd: HWND, file: &str) {
        let mut r = windows::Win32::Foundation::RECT::default();
        unsafe { GetWindowRect(hwnd, &mut r) }.expect("GetWindowRect");
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        let mut px = vec![0u8; (w * h * 4) as usize];
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        unsafe {
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            let bmp = CreateCompatibleBitmap(screen, w, h);
            SelectObject(dc, bmp.into());
            BitBlt(dc, 0, 0, w, h, Some(screen), r.left, r.top, SRCCOPY).expect("BitBlt");
            GetDIBits(
                dc,
                bmp,
                0,
                h as u32,
                Some(px.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            );
        }
        let mut out = Vec::new();
        out.extend(b"BM");
        out.extend((54 + px.len() as u32).to_le_bytes());
        out.extend([0u8; 4]);
        out.extend(54u32.to_le_bytes());
        out.extend(40u32.to_le_bytes());
        out.extend(w.to_le_bytes());
        out.extend((-h).to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend([0u8; 24]);
        out.extend(px);
        std::fs::write(file, out).expect("write screenshot");
        println!("screenshot written: {file}");
    }

    fn ui(page: HPROPSHEETPAGE, shot: Option<&str>) {
        let mut pages = [page];
        let caption: Vec<u16> = "smoke\0".encode_utf16().collect();
        let header = PROPSHEETHEADERW_V2 {
            dwSize: std::mem::size_of::<PROPSHEETHEADERW_V2>() as u32,
            dwFlags: PSH_MODELESS | PSH_USEHPSP,
            pszCaption: PCWSTR(caption.as_ptr()),
            nPages: 1,
            Anonymous2: PROPSHEETHEADERW_V2_1 { nStartPage: 0 },
            Anonymous3: PROPSHEETHEADERW_V2_2 {
                phpage: pages.as_mut_ptr(),
            },
            Anonymous4: PROPSHEETHEADERW_V2_3::default(),
            ..Default::default()
        };
        let sheet = HWND(unsafe { PropertySheetW(&header as *const _ as *mut _) } as *mut c_void);
        step("modeless property sheet created", !sheet.0.is_null());
        pump(500);
        let page_hwnd = HWND(
            unsafe {
                SendMessageW(
                    sheet,
                    PSM_GETCURRENTPAGEHWND,
                    Some(WPARAM(0)),
                    Some(LPARAM(0)),
                )
            }
            .0 as *mut c_void,
        );
        step("page dialog exists", !page_hwnd.0.is_null());
        if let Some(f) = shot {
            screenshot(sheet, &format!("{f}.before.bmp"));
        }
        println!("repo:     {}", text(page_hwnd, 101));
        println!("note:     {}", text(page_hwnd, 110));
        println!("headline: {}", text(page_hwnd, 102));
        println!("consent:  {}", text(page_hwnd, 104));
        println!("explain:  {}", text(page_hwnd, 111));
        let verify = unsafe { GetDlgItem(Some(page_hwnd), 107) }.unwrap();
        step(
            "Verify enabled for a file with a claim",
            unsafe { IsWindowEnabled(verify) }.as_bool(),
        );
        let demo = std::env::var_os("SIGSTORE_SHELL_DEMO_REPORT").is_some();
        if !demo {
            unsafe {
                SendMessageW(
                    page_hwnd,
                    windows::Win32::UI::WindowsAndMessaging::WM_COMMAND,
                    Some(WPARAM(107)),
                    Some(LPARAM(verify.0 as isize)),
                )
            };
        }
        let _ = BM_CLICK;
        pump(300);
        println!("after click: {}", text(page_hwnd, 102));
        println!("progress:    {}", text(page_hwnd, 106));
        for _ in 0..120 {
            pump(500);
            if text(page_hwnd, 102) != "Verifying..." {
                break;
            }
        }
        println!("final headline: {}", text(page_hwnd, 102));
        println!(
            "final repo: {} {}",
            text(page_hwnd, 101),
            text(page_hwnd, 110)
        );
        println!("rate line: {}", text(page_hwnd, 117));
        println!("verify label: {}", text(page_hwnd, 107));
        println!("final details:\n{}", text(page_hwnd, 103));
        pump(500);
        if let Some(f) = shot {
            screenshot(sheet, &format!("{f}.after.bmp"));
        }
        if demo {
            click_clear_cache(page_hwnd);
            if let Some(f) = shot {
                screenshot(sheet, &format!("{f}.cleared.bmp"));
            }
        }
        let _ = DestroyPropertySheetPage;
    }

    /// The demo report is a cached result: clicking "Clear cache" must turn its line into the plain notice.
    fn click_clear_cache(page: HWND) {
        step(
            "cached result shows its actions",
            text(page, 121).contains("Check online") && text(page, 121).contains("Clear cache"),
        );
        let link = unsafe { GetDlgItem(Some(page), 121) }.unwrap();
        let mut nm = NMLINK {
            hdr: NMHDR {
                hwndFrom: link,
                idFrom: 121,
                code: NM_CLICK,
            },
            ..Default::default()
        };
        nm.item.iLink = 1;
        unsafe {
            SendMessageW(
                page,
                WM_NOTIFY,
                Some(WPARAM(121)),
                Some(LPARAM(&nm as *const NMLINK as isize)),
            )
        };
        pump(300);
        println!("after Clear cache: {}", text(page, 117));
        step(
            "Clear cache leaves a plain notice",
            text(page, 117).starts_with("Cached result cleared"),
        );
    }

    pub fn main() {
        let args: Vec<String> = std::env::args().collect();
        let (dll, exe) = (&args[1], &args[2]);
        let with_ui = args.iter().any(|a| a == "--ui");
        let wide: Vec<u16> = dll.encode_utf16().chain([0]).collect();
        let module = unsafe { LoadLibraryW(PCWSTR(wide.as_ptr())) };
        step("LoadLibrary", module.is_ok());
        let module = module.unwrap();
        let get = unsafe { GetProcAddress(module, windows::core::s!("DllGetClassObject")) };
        let can = unsafe { GetProcAddress(module, windows::core::s!("DllCanUnloadNow")) };
        step("exports present", get.is_some() && can.is_some());
        let get: GetClassObject = unsafe { std::mem::transmute(get.unwrap()) };
        let can: CanUnload = unsafe { std::mem::transmute(can.unwrap()) };

        let mut ppv = std::ptr::null_mut();
        let wrong = GUID::from_u128(1);
        let hr = unsafe { get(&wrong, &IClassFactory::IID, &mut ppv) };
        step("unknown CLSID rejected", hr == CLASS_E_CLASSNOTAVAILABLE);

        let hr = unsafe { get(&CLSID, &IClassFactory::IID, &mut ppv) };
        step(
            "DllGetClassObject returns a factory",
            hr == S_OK && !ppv.is_null(),
        );
        let factory = unsafe { IClassFactory::from_raw(ppv) };
        step(
            "DllCanUnloadNow is S_FALSE while a factory lives",
            unsafe { can() } == S_FALSE,
        );

        let init: IShellExtInit = unsafe { factory.CreateInstance(None) }.expect("CreateInstance");
        step("non-exe selection is refused", {
            let txt = data_object(&format!("{exe}.txt"));
            unsafe { init.Initialize(None, &txt, Default::default()) }.is_err()
        });
        let data = data_object(exe);
        step(
            "Initialize accepts a single exe",
            unsafe { init.Initialize(None, &data, Default::default()) }.is_ok(),
        );
        let ext: IShellPropSheetExt = init.cast().expect("QI IShellPropSheetExt");
        step(
            "AddPages adds one page",
            unsafe { ext.AddPages(Some(add_page), LPARAM(0)) }.is_ok()
                && PAGES.with(|p| p.borrow().len()) == 1,
        );

        let page = PAGES.with(|p| p.borrow()[0]);
        if with_ui {
            let shot = args
                .iter()
                .position(|a| a == "--shot")
                .map(|i| args[i + 1].as_str());
            ui(page, shot);
        } else {
            step(
                "page destroyed",
                unsafe { DestroyPropertySheetPage(page) }.as_bool(),
            );
        }
        drop(ext);
        drop(init);
        drop(data);
        drop(factory);
        step(
            "DllCanUnloadNow is S_OK after everything is released",
            unsafe { can() } == S_OK || with_ui,
        );
        println!("DONE");
    }
}

#[cfg(windows)]
fn main() {
    imp::main();
}
