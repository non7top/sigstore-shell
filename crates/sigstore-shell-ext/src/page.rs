use crate::dlgtemplate::{
    build, FOOTER_RECT, IDC_CANCEL, IDC_CLAIM, IDC_CONSENT, IDC_COPY_SHA, IDC_COPY_SIGNER,
    IDC_DETAILS, IDC_DETAILS_LABEL, IDC_FOOTER, IDC_GLYPH, IDC_HEADLINE, IDC_LINKS, IDC_PROGRESS,
    IDC_PROGRESS_TEXT, IDC_SUMMARY, IDC_VERIFY, LINKS_RECT,
};
use crate::dll::{guard_value, module, Live};
use crate::links::{openable, strip_markup, FOOTER_MARKUP, FOOTER_URLS};
use crate::model::{view, Claim, Event, Outcome, State, View};
use crate::settings::{app_dir, Settings};
use crate::worker::{self, JobCtx};
use provenance_core::read_claim_file;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::{
    GlobalFree, COLORREF, E_FAIL, HANDLE, HINSTANCE, HWND, LPARAM, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, DeleteObject, GetDC, GetDeviceCaps, GetStockObject, GetSysColor, InvalidateRect,
    ReleaseDC, SetBkMode, SetTextColor, CLIP_DEFAULT_PRECIS, COLOR_GRAYTEXT, COLOR_WINDOWTEXT,
    DEFAULT_CHARSET, DEFAULT_QUALITY, FF_DONTCARE, FW_BOLD, FW_NORMAL, HDC, HFONT, HGDIOBJ,
    LOGPIXELSY, NULL_BRUSH, OUT_DEFAULT_PRECIS, TRANSPARENT,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::Controls::{
    CreatePropertySheetPageW, HPROPSHEETPAGE, NMHDR, NMLINK, NM_CLICK, NM_RETURN, PBM_SETMARQUEE,
    PROPSHEETPAGEW, PROPSHEETPAGEW_0, PSPCB_MESSAGE, PSPCB_RELEASE, PSP_DLGINDIRECT,
    PSP_USECALLBACK, PSP_USETITLE,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetDlgCtrlID, GetDlgItem, GetWindowLongPtrW, MapDialogRect, PostMessageW,
    SendMessageW, SetWindowLongPtrW, SetWindowTextW, ShowWindow, SystemParametersInfoW,
    DLGTEMPLATE, HMENU, SPI_GETHIGHCONTRAST, SW_HIDE, SW_SHOW, SW_SHOWNORMAL,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE,
    WM_APP, WM_COMMAND, WM_CTLCOLORSTATIC, WM_DESTROY, WM_GETFONT, WM_INITDIALOG, WM_NOTIFY,
    WM_SETFONT, WS_CHILD, WS_TABSTOP,
};

/// DWLP_USER in winuser.h (DWLP_DLGPROC + pointer size); the windows crate only has the 32-bit DWL_USER.
const DWLP_USER: WINDOW_LONG_PTR_INDEX = WINDOW_LONG_PTR_INDEX(16);

const WM_APP_UPDATE: u32 = WM_APP + 1;
const BN_CLICKED: u32 = 0;
const EM_SETSEL: u32 = 0xB1;
const CF_UNICODETEXT: u32 = 13;
const LWS_TRANSPARENT: u32 = 0x0001;

/// What the worker thread may touch; the rest of the page state stays on the UI thread.
pub struct Mailbox {
    hwnd: AtomicIsize,
    events: Mutex<Vec<Event>>,
}

impl Mailbox {
    pub fn post(&self, event: Event) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
        let hwnd = self.hwnd.load(Ordering::Acquire);
        if hwnd != 0 {
            let _ = unsafe {
                PostMessageW(
                    Some(HWND(hwnd as *mut _)),
                    WM_APP_UPDATE,
                    WPARAM(0),
                    LPARAM(0),
                )
            };
        }
    }

    fn drain(&self) -> Vec<Event> {
        self.events
            .lock()
            .map(|mut e| std::mem::take(&mut *e))
            .unwrap_or_default()
    }
}

struct Ui {
    claim: Claim,
    rekor: bool,
    state: State,
    cancel: Arc<AtomicBool>,
}

struct PageData {
    path: PathBuf,
    /// Must outlive the page: the system reads it when the page is first shown.
    template: Vec<u8>,
    mailbox: Arc<Mailbox>,
    next_job: AtomicU64,
    ui: RefCell<Ui>,
    outcome: Cell<Outcome>,
    /// Symbol and monospace fonts, created on init and freed on destroy.
    fonts: Cell<[isize; 2]>,
    has_link_class: Cell<bool>,
    _live: Live,
}

pub fn create(path: PathBuf) -> Result<HPROPSHEETPAGE> {
    let data = Box::into_raw(Box::new(PageData {
        path,
        template: build(),
        mailbox: Arc::new(Mailbox {
            hwnd: AtomicIsize::new(0),
            events: Mutex::new(Vec::new()),
        }),
        next_job: AtomicU64::new(0),
        ui: RefCell::new(Ui {
            claim: Claim::Absent,
            rekor: false,
            state: State::Idle,
            cancel: Arc::new(AtomicBool::new(false)),
        }),
        outcome: Cell::new(Outcome::None),
        fonts: Cell::new([0; 2]),
        has_link_class: Cell::new(false),
        _live: Live::new(),
    }));
    let mut psp = PROPSHEETPAGEW {
        dwSize: std::mem::size_of::<PROPSHEETPAGEW>() as u32,
        dwFlags: PSP_DLGINDIRECT | PSP_USETITLE | PSP_USECALLBACK,
        hInstance: HINSTANCE(module().0),
        Anonymous1: PROPSHEETPAGEW_0 {
            pResource: unsafe { (*data).template.as_ptr().cast_mut().cast::<DLGTEMPLATE>() },
        },
        pszTitle: w!("Sigstore"),
        pfnDlgProc: Some(dlg_proc),
        lParam: LPARAM(data as isize),
        pfnCallback: Some(page_callback),
        ..Default::default()
    };
    let page = unsafe { CreatePropertySheetPageW(&mut psp) };
    if page.is_invalid() {
        drop(unsafe { Box::from_raw(data) });
        return Err(E_FAIL.into());
    }
    Ok(page)
}

unsafe extern "system" fn page_callback(
    _hwnd: HWND,
    msg: PSPCB_MESSAGE,
    psp: *mut PROPSHEETPAGEW,
) -> u32 {
    if msg == PSPCB_RELEASE {
        drop(Box::from_raw((*psp).lParam.0 as *mut PageData));
    }
    1
}

unsafe fn data_of<'a>(hwnd: HWND) -> Option<&'a PageData> {
    (GetWindowLongPtrW(hwnd, DWLP_USER) as *const PageData).as_ref()
}

unsafe fn set_text(hwnd: HWND, id: u16, text: &str) {
    if let Ok(item) = GetDlgItem(Some(hwnd), i32::from(id)) {
        let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
        let _ = SetWindowTextW(item, windows::core::PCWSTR(wide.as_ptr()));
    }
}

unsafe fn show(hwnd: HWND, id: u16, visible: bool) {
    if let Ok(item) = GetDlgItem(Some(hwnd), i32::from(id)) {
        let _ = ShowWindow(item, if visible { SW_SHOW } else { SW_HIDE });
    }
}

unsafe fn set_enabled(hwnd: HWND, id: u16, enabled: bool) {
    if let Ok(item) = GetDlgItem(Some(hwnd), i32::from(id)) {
        let _ = EnableWindow(item, enabled);
    }
}

unsafe fn high_contrast() -> bool {
    let mut hc = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    SystemParametersInfoW(
        SPI_GETHIGHCONTRAST,
        hc.cbSize,
        Some((&mut hc as *mut HIGHCONTRASTW).cast()),
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    )
    .is_ok()
        && hc.dwFlags.0 & HCF_HIGHCONTRASTON.0 != 0
}

/// Colour only reinforces the glyph and the verdict text; high contrast uses the system text colour.
unsafe fn outcome_color(outcome: Outcome) -> COLORREF {
    if high_contrast() {
        return COLORREF(GetSysColor(COLOR_WINDOWTEXT));
    }
    match outcome {
        Outcome::Good => COLORREF(0x0010_7C10),
        Outcome::Bad => COLORREF(0x001C_2BC4),
        _ => COLORREF(GetSysColor(COLOR_GRAYTEXT)),
    }
}

unsafe fn make_font(dpi: i32, points: i32, weight: i32, face: PCWSTR) -> HFONT {
    CreateFontW(
        -(points * dpi / 72),
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        DEFAULT_QUALITY,
        FF_DONTCARE.0 as u32,
        face,
    )
}

unsafe fn set_font(hwnd: HWND, id: u16, font: isize) {
    if let Ok(item) = GetDlgItem(Some(hwnd), i32::from(id)) {
        SendMessageW(
            item,
            WM_SETFONT,
            Some(WPARAM(font as usize)),
            Some(LPARAM(1)),
        );
    }
}

unsafe fn create_fonts(hwnd: HWND, data: &PageData) {
    let dc = GetDC(Some(hwnd));
    let dpi = GetDeviceCaps(Some(dc), LOGPIXELSY).max(96);
    ReleaseDC(Some(hwnd), dc);
    let symbol = make_font(dpi, 14, FW_BOLD.0 as i32, w!("Segoe UI Symbol"));
    let mono = make_font(dpi, 8, FW_NORMAL.0 as i32, w!("Consolas"));
    data.fonts.set([symbol.0 as isize, mono.0 as isize]);
    set_font(hwnd, IDC_GLYPH, symbol.0 as isize);
    set_font(hwnd, IDC_DETAILS, mono.0 as isize);
}

unsafe fn free_fonts(data: &PageData) {
    for f in data.fonts.replace([0; 2]) {
        if f != 0 {
            let _ = DeleteObject(HGDIOBJ(f as *mut _));
        }
    }
}

/// SysLink needs comctl32 v6; where it is missing the page still opens, with plain text instead.
unsafe fn create_text(
    hwnd: HWND,
    id: u16,
    rect: (i16, i16, i16, i16),
    markup: &str,
    data: &PageData,
) {
    let mut r = RECT {
        left: i32::from(rect.0),
        top: i32::from(rect.1),
        right: i32::from(rect.0) + i32::from(rect.2),
        bottom: i32::from(rect.1) + i32::from(rect.3),
    };
    let _ = MapDialogRect(hwnd, &mut r);
    let make = |class: PCWSTR, text: &str, style: u32| {
        let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            PCWSTR(wide.as_ptr()),
            WINDOW_STYLE(WS_CHILD.0 | style),
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            Some(hwnd),
            Some(HMENU(usize::from(id) as *mut _)),
            Some(HINSTANCE(module().0)),
            None,
        )
    };
    let link = make(w!("SysLink"), markup, WS_TABSTOP.0 | LWS_TRANSPARENT);
    let (item, ok) = match link {
        Ok(h) => (Some(h), true),
        Err(_) if id == IDC_FOOTER => (make(w!("STATIC"), &strip_markup(markup), 0x80).ok(), false),
        Err(_) => (None, false),
    };
    if id == IDC_LINKS {
        data.has_link_class.set(ok);
    }
    if let Some(item) = item {
        let font = SendMessageW(hwnd, WM_GETFONT, None, None).0;
        SendMessageW(
            item,
            WM_SETFONT,
            Some(WPARAM(font as usize)),
            Some(LPARAM(1)),
        );
        if id == IDC_FOOTER {
            let _ = ShowWindow(item, SW_SHOW);
        }
    }
}

unsafe fn invalidate_item(hwnd: HWND, id: u16) {
    if let Ok(item) = GetDlgItem(Some(hwnd), i32::from(id)) {
        let mut r = RECT::default();
        if windows::Win32::UI::WindowsAndMessaging::GetWindowRect(item, &mut r).is_ok() {
            let mut pts = [
                windows::Win32::Foundation::POINT {
                    x: r.left,
                    y: r.top,
                },
                windows::Win32::Foundation::POINT {
                    x: r.right,
                    y: r.bottom,
                },
            ];
            windows::Win32::Graphics::Gdi::MapWindowPoints(None, Some(hwnd), &mut pts);
            let r = RECT {
                left: pts[0].x,
                top: pts[0].y,
                right: pts[1].x,
                bottom: pts[1].y,
            };
            let _ = InvalidateRect(Some(hwnd), Some(&r), true);
        }
    }
}

unsafe fn copy_to_clipboard(hwnd: HWND, text: &str) {
    if OpenClipboard(Some(hwnd)).is_err() {
        return;
    }
    let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
    if let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
        let ptr = GlobalLock(mem).cast::<u16>();
        let placed = if ptr.is_null() {
            false
        } else {
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
            let _ = GlobalUnlock(mem);
            let _ = EmptyClipboard();
            SetClipboardData(CF_UNICODETEXT, Some(HANDLE(mem.0))).is_ok()
        };
        if !placed {
            let _ = GlobalFree(Some(mem));
        }
    }
    let _ = CloseClipboard();
}

unsafe fn open_url(hwnd: HWND, url: &str) {
    if !openable(url) {
        return;
    }
    let wide: Vec<u16> = url.encode_utf16().chain([0]).collect();
    ShellExecuteW(
        Some(hwnd),
        w!("open"),
        PCWSTR(wide.as_ptr()),
        None,
        None,
        SW_SHOWNORMAL,
    );
}

unsafe fn render(hwnd: HWND, data: &PageData) {
    // Snapshot first: setting control text can notify us again while the cell is borrowed.
    let v: View = {
        let ui = data.ui.borrow();
        view(&ui.claim, &ui.state, ui.rekor)
    };
    set_text(hwnd, IDC_CLAIM, &v.claim_line);
    if data.outcome.replace(v.outcome) != v.outcome {
        set_text(hwnd, IDC_GLYPH, v.outcome.glyph());
        invalidate_item(hwnd, IDC_GLYPH);
    }
    set_text(hwnd, IDC_HEADLINE, &v.headline);
    set_text(hwnd, IDC_SUMMARY, &v.summary);
    let markup = v.links_markup();
    set_text(hwnd, IDC_LINKS, &markup);
    show(
        hwnd,
        IDC_LINKS,
        !markup.is_empty() && data.has_link_class.get(),
    );
    let details = v.has_details();
    set_text(
        hwnd,
        IDC_DETAILS_LABEL,
        if details {
            "From the signing certificate:"
        } else {
            ""
        },
    );
    show(hwnd, IDC_DETAILS_LABEL, details);
    show(hwnd, IDC_DETAILS, details);
    set_text(hwnd, IDC_DETAILS, &v.details_text());
    if let Ok(edit) = GetDlgItem(Some(hwnd), i32::from(IDC_DETAILS)) {
        SendMessageW(edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(0)));
    }
    show(hwnd, IDC_COPY_SHA, v.sha256.is_some());
    show(hwnd, IDC_COPY_SIGNER, v.signer.is_some());
    set_text(hwnd, IDC_CONSENT, &v.consent);
    show(hwnd, IDC_CONSENT, v.show_consent);
    set_text(hwnd, IDC_VERIFY, v.verify_label);
    set_text(hwnd, IDC_PROGRESS_TEXT, &v.progress_text);
    show(hwnd, IDC_PROGRESS, v.running);
    show(hwnd, IDC_CANCEL, v.running);
    if let Ok(bar) = GetDlgItem(Some(hwnd), i32::from(IDC_PROGRESS)) {
        SendMessageW(
            bar,
            PBM_SETMARQUEE,
            Some(WPARAM(usize::from(v.running))),
            Some(LPARAM(30)),
        );
    }
    set_enabled(hwnd, IDC_VERIFY, v.can_verify);
    let focus = if v.running { IDC_CANCEL } else { IDC_VERIFY };
    if v.running || v.can_verify {
        if let Ok(item) = GetDlgItem(Some(hwnd), i32::from(focus)) {
            let _ = SetFocus(Some(item));
        }
    }
}

unsafe fn on_init(hwnd: HWND, psp: *const PROPSHEETPAGEW) {
    let data = (*psp).lParam.0 as *const PageData;
    SetWindowLongPtrW(hwnd, DWLP_USER, data as isize);
    let data = &*data;
    data.mailbox.hwnd.store(hwnd.0 as isize, Ordering::Release);
    {
        let mut ui = data.ui.borrow_mut();
        ui.claim = Claim::from_read(read_claim_file(&data.path));
        ui.rekor = app_dir()
            .map(|d| Settings::load(&d))
            .unwrap_or_default()
            .rekor;
    }
    create_fonts(hwnd, data);
    create_text(hwnd, IDC_LINKS, LINKS_RECT, "", data);
    create_text(hwnd, IDC_FOOTER, FOOTER_RECT, FOOTER_MARKUP, data);
    render(hwnd, data);
}

unsafe fn on_copy(hwnd: HWND, data: &PageData, id: u16) {
    let text = {
        let ui = data.ui.borrow();
        let v = view(&ui.claim, &ui.state, ui.rekor);
        if id == IDC_COPY_SHA {
            v.sha256
        } else {
            v.signer
        }
    };
    if let Some(text) = text {
        copy_to_clipboard(hwnd, &text);
    }
}

unsafe fn on_link(hwnd: HWND, data: &PageData, nm: *const NMHDR) {
    if (*nm).code != NM_CLICK && (*nm).code != NM_RETURN {
        return;
    }
    let index = usize::try_from((*nm.cast::<NMLINK>()).item.iLink).unwrap_or(usize::MAX);
    let url = match (*nm).idFrom {
        id if id == usize::from(IDC_FOOTER) => FOOTER_URLS.get(index).map(|u| (*u).to_string()),
        id if id == usize::from(IDC_LINKS) => {
            let ui = data.ui.borrow();
            view(&ui.claim, &ui.state, ui.rekor)
                .links
                .get(index)
                .map(|l| l.url.clone())
        }
        _ => None,
    };
    if let Some(url) = url {
        open_url(hwnd, &url);
    }
}

unsafe fn glyph_brush(hdc: HDC, data: &PageData) -> isize {
    SetTextColor(hdc, outcome_color(data.outcome.get()));
    SetBkMode(hdc, TRANSPARENT);
    GetStockObject(NULL_BRUSH).0 as isize
}

unsafe fn on_verify(hwnd: HWND, data: &PageData) {
    let ctx = {
        let mut ui = data.ui.borrow_mut();
        if ui.state.is_running() || (ui.claim.repo().is_none() && !ui.rekor) {
            return;
        }
        let job = data.next_job.fetch_add(1, Ordering::Relaxed) + 1;
        ui.cancel = Arc::new(AtomicBool::new(false));
        ui.state = std::mem::replace(&mut ui.state, State::Idle).apply(Event::Start { job });
        JobCtx {
            job,
            path: data.path.clone(),
            claimed_repo: ui.claim.repo().map(str::to_string),
            rekor: ui.rekor,
            cancel: ui.cancel.clone(),
            mailbox: data.mailbox.clone(),
        }
    };
    let job = ctx.job;
    if let Err(e) = worker::spawn(ctx) {
        data.mailbox.post(Event::Finished {
            job,
            result: Err(format!("could not start the verification thread: {e}")),
        });
    }
    render(hwnd, data);
}

unsafe fn on_cancel(hwnd: HWND, data: &PageData) {
    {
        let mut ui = data.ui.borrow_mut();
        ui.cancel.store(true, Ordering::Relaxed);
        ui.state = std::mem::replace(&mut ui.state, State::Idle).apply(Event::Cancel);
    }
    render(hwnd, data);
}

unsafe fn on_update(hwnd: HWND, data: &PageData) {
    let events = data.mailbox.drain();
    {
        let mut ui = data.ui.borrow_mut();
        for event in events {
            ui.state = std::mem::replace(&mut ui.state, State::Idle).apply(event);
        }
    }
    render(hwnd, data);
}

unsafe extern "system" fn dlg_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> isize {
    guard_value(0, || {
        match msg {
            WM_INITDIALOG => {
                on_init(hwnd, lparam.0 as *const PROPSHEETPAGEW);
                return 0;
            }
            WM_COMMAND => {
                let id = (wparam.0 & 0xFFFF) as u16;
                let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
                if let (Some(data), BN_CLICKED) = (data_of(hwnd), code) {
                    match id {
                        IDC_VERIFY => on_verify(hwnd, data),
                        IDC_CANCEL => on_cancel(hwnd, data),
                        IDC_COPY_SHA | IDC_COPY_SIGNER => on_copy(hwnd, data, id),
                        _ => {}
                    }
                }
            }
            WM_NOTIFY => {
                if let Some(data) = data_of(hwnd) {
                    on_link(hwnd, data, lparam.0 as *const NMHDR);
                }
            }
            WM_CTLCOLORSTATIC => {
                let ctl = HWND(lparam.0 as *mut _);
                if let Some(data) = data_of(hwnd) {
                    if GetDlgCtrlID(ctl) == i32::from(IDC_GLYPH) {
                        return glyph_brush(HDC(wparam.0 as *mut _), data);
                    }
                }
            }
            WM_APP_UPDATE => {
                if let Some(data) = data_of(hwnd) {
                    on_update(hwnd, data);
                }
            }
            WM_DESTROY => {
                if let Some(data) = data_of(hwnd) {
                    data.mailbox.hwnd.store(0, Ordering::Release);
                    data.ui.borrow().cancel.store(true, Ordering::Relaxed);
                    free_fonts(data);
                }
                SetWindowLongPtrW(hwnd, DWLP_USER, 0);
            }
            _ => {}
        }
        0
    })
}
