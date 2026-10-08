use crate::cache::Cache;
use crate::dlgtemplate::{
    build, CACHE_RECT, CONTENT_WIDTH, FOOTER_RECT, ICON_FAILED, ICON_NEUTRAL, ICON_SLOT,
    ICON_VERIFIED, ICON_WARNING, IDC_CACHE, IDC_CANCEL, IDC_CONSENT, IDC_COPY_SHA, IDC_COPY_SIGNER,
    IDC_DETAILS, IDC_EXPLAIN, IDC_FOOTER, IDC_GLYPH, IDC_HEADLINE, IDC_ICON, IDC_LINKS, IDC_LINKS2,
    IDC_PROGRESS, IDC_PROGRESS_TEXT, IDC_RATE, IDC_REPO, IDC_REPO_NOTE, IDC_REPO_SUFFIX,
    IDC_VERIFY, LINKS_RECT, MARGIN, REPO_ROW, REPO_WIDTH, VERDICT_ROW,
};
use crate::dll::{guard_value, module, Live};
use crate::links::{
    cache_markup, markup, openable, split_rows, strip_markup, Link, FOOTER_MARKUP, FOOTER_URLS,
};
use crate::model::{view_in, Claim, Env, Event, Outcome, Rate, State, Tone, View};
use crate::settings::{app_dir, Settings};
use crate::worker::{self, JobCtx};
use provenance_core::read_claim_file;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use windows::core::{w, Result, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    GlobalFree, COLORREF, E_FAIL, HANDLE, HINSTANCE, HWND, LPARAM, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    CreateFontIndirectW, CreateFontW, DeleteObject, DrawTextW, GetDC, GetDeviceCaps, GetObjectW,
    GetStockObject, GetSysColor, GetTextExtentPoint32W, GetTextMetricsW, InvalidateRect,
    RedrawWindow, ReleaseDC, SelectObject, SetBkMode, SetTextColor, CLIP_DEFAULT_PRECIS,
    COLOR_GRAYTEXT, COLOR_WINDOWTEXT, DEFAULT_CHARSET, DEFAULT_QUALITY, DT_CALCRECT, DT_NOPREFIX,
    DT_WORDBREAK, FF_DONTCARE, FW_BOLD, FW_NORMAL, HDC, HFONT, HGDIOBJ, LOGFONTW, LOGPIXELSY,
    NULL_BRUSH, OUT_DEFAULT_PRECIS, RDW_ALLCHILDREN, RDW_ERASE, RDW_INVALIDATE, RDW_UPDATENOW,
    TEXTMETRICW, TRANSPARENT,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::Controls::{
    CreatePropertySheetPageW, DrawThemeParentBackground, DRAWITEMSTRUCT, HPROPSHEETPAGE, NMHDR,
    NMLINK, NM_CLICK, NM_RETURN, PBM_SETMARQUEE, PROPSHEETPAGEW, PROPSHEETPAGEW_0, PSPCB_MESSAGE,
    PSPCB_RELEASE, PSP_DLGINDIRECT, PSP_USECALLBACK, PSP_USETITLE, TTF_IDISHWND, TTF_SUBCLASS,
    TTM_ADDTOOLW, TTM_SETMAXTIPWIDTH, TTTOOLINFOW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetDlgCtrlID, GetDlgItem, GetWindowLongPtrW, MapDialogRect, PostMessageW,
    SendMessageW, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow,
    SystemParametersInfoW, DLGTEMPLATE, HMENU, SPI_GETHIGHCONTRAST, SWP_NOACTIVATE, SWP_NOZORDER,
    SW_HIDE, SW_SHOW, SW_SHOWNORMAL, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WINDOW_EX_STYLE,
    WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_APP, WM_COMMAND, WM_CTLCOLORSTATIC, WM_DESTROY,
    WM_GETFONT, WM_INITDIALOG, WM_NOTIFY, WM_SETFONT, WS_CHILD, WS_POPUP, WS_TABSTOP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, DrawIconEx, LoadImageW, DI_NORMAL, HICON, IMAGE_ICON, LR_DEFAULTCOLOR, WM_DRAWITEM,
};

/// DWLP_USER in winuser.h (DWLP_DLGPROC + pointer size); the windows crate only has the 32-bit DWL_USER.
const DWLP_USER: WINDOW_LONG_PTR_INDEX = WINDOW_LONG_PTR_INDEX(16);

const WM_APP_UPDATE: u32 = WM_APP + 1;
const BN_CLICKED: u32 = 0;
const EM_SETSEL: u32 = 0xB1;
const EM_GETLINECOUNT: u32 = 0xBA;
const CF_UNICODETEXT: u32 = 13;
const LWS_TRANSPARENT: u32 = 0x0001;
const TTS_ALWAYSTIP: u32 = 0x01;

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
    /// From the last fresh response in this tab only; nothing is stored.
    rate: Option<Rate>,
    tz: jiff::tz::TimeZone,
}

impl Ui {
    fn view(&self) -> View {
        let env = Env {
            rate: self.rate,
            tz: self.tz.clone(),
        };
        view_in(&self.claim, &self.state, self.rekor, &env)
    }
}

struct PageData {
    path: PathBuf,
    /// Must outlive the page: the system reads it when the page is first shown.
    template: Vec<u8>,
    mailbox: Arc<Mailbox>,
    next_job: AtomicU64,
    ui: RefCell<Ui>,
    outcome: Cell<Outcome>,
    tone: Cell<Tone>,
    shape: Cell<u64>,
    /// Symbol, monospace, bold and small fonts, created on init and freed on destroy.
    fonts: Cell<[isize; 4]>,
    has_link_class: Cell<bool>,
    /// How many links sit in the first row; the rest are in the second.
    link_split: Cell<usize>,
    icon: Cell<isize>,
    icon_px: Cell<i32>,
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
            rate: None,
            tz: jiff::tz::TimeZone::UTC,
        }),
        outcome: Cell::new(Outcome::None),
        tone: Cell::new(Tone::Normal),
        shape: Cell::new(0),
        fonts: Cell::new([0; 4]),
        has_link_class: Cell::new(false),
        link_split: Cell::new(0),
        icon: Cell::new(0),
        icon_px: Cell::new(20),
        _live: Live::new(),
    }));
    let mut psp = PROPSHEETPAGEW {
        dwSize: std::mem::size_of::<PROPSHEETPAGEW>() as u32,
        dwFlags: PSP_DLGINDIRECT | PSP_USETITLE | PSP_USECALLBACK,
        hInstance: HINSTANCE(module().0),
        Anonymous1: PROPSHEETPAGEW_0 {
            pResource: unsafe { (*data).template.as_ptr().cast_mut().cast::<DLGTEMPLATE>() },
        },
        pszTitle: w!("Provenance"),
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

/// Bold and slightly smaller copies of the dialog font, so they follow the system font and DPI.
unsafe fn derived_fonts(hwnd: HWND) -> (isize, isize) {
    let base = SendMessageW(hwnd, WM_GETFONT, None, None).0;
    let mut lf = LOGFONTW::default();
    if base == 0
        || GetObjectW(
            HGDIOBJ(base as *mut _),
            std::mem::size_of::<LOGFONTW>() as i32,
            Some((&mut lf as *mut LOGFONTW).cast()),
        ) == 0
    {
        return (0, 0);
    }
    let mut bold = lf;
    bold.lfWeight = FW_BOLD.0 as i32;
    let mut small = lf;
    small.lfHeight = lf.lfHeight * 9 / 10;
    (
        CreateFontIndirectW(&bold).0 as isize,
        CreateFontIndirectW(&small).0 as isize,
    )
}

unsafe fn create_fonts(hwnd: HWND, data: &PageData) {
    let dc = GetDC(Some(hwnd));
    let dpi = GetDeviceCaps(Some(dc), LOGPIXELSY).max(96);
    ReleaseDC(Some(hwnd), dc);
    let symbol = make_font(dpi, 14, FW_BOLD.0 as i32, w!("Segoe UI Symbol"));
    let mono = make_font(dpi, 8, FW_NORMAL.0 as i32, w!("Consolas"));
    let (bold, small) = derived_fonts(hwnd);
    data.fonts
        .set([symbol.0 as isize, mono.0 as isize, bold, small]);
    set_font(hwnd, IDC_GLYPH, symbol.0 as isize);
    set_font(hwnd, IDC_DETAILS, mono.0 as isize);
    set_font(hwnd, IDC_REPO, bold);
    for id in [IDC_EXPLAIN, IDC_CONSENT, IDC_RATE] {
        set_font(hwnd, id, small);
    }
}

unsafe fn free_fonts(data: &PageData) {
    for f in data.fonts.replace([0; 4]) {
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

fn icon_id(outcome: Outcome) -> Option<u16> {
    match outcome {
        Outcome::None => None,
        Outcome::Good => Some(ICON_VERIFIED),
        Outcome::Bad => Some(ICON_FAILED),
        Outcome::Neutral => Some(ICON_NEUTRAL),
        Outcome::Warn => Some(ICON_WARNING),
    }
}

/// Shows the outcome icon, or the coloured text glyph when high contrast is on or the icon will not load.
/// Owner-drawn so the 32-bit icon is blended onto the real page background; a static icon control
/// paints into a zero-filled (black) buffer when its brush does not cover the background.
unsafe fn draw_icon(data: &PageData, item: &DRAWITEMSTRUCT) {
    let _ = DrawThemeParentBackground(item.hwndItem, item.hDC, None);
    let icon = data.icon.get();
    if icon != 0 {
        let px = data.icon_px.get();
        let _ = DrawIconEx(
            item.hDC,
            0,
            0,
            HICON(icon as *mut _),
            px,
            px,
            0,
            None,
            DI_NORMAL,
        );
    }
}

unsafe fn show_outcome(hwnd: HWND, data: &PageData, outcome: Outcome) {
    let mut icon = HICON::default();
    if let (Some(id), false) = (icon_id(outcome), high_contrast()) {
        let dc = GetDC(Some(hwnd));
        let dpi = GetDeviceCaps(Some(dc), LOGPIXELSY).max(96);
        ReleaseDC(Some(hwnd), dc);
        let px = 20 * dpi / 96;
        data.icon_px.set(px);
        if let Ok(h) = LoadImageW(
            Some(HINSTANCE(module().0)),
            PCWSTR(usize::from(id) as *const u16),
            IMAGE_ICON,
            px,
            px,
            LR_DEFAULTCOLOR,
        ) {
            icon = HICON(h.0);
        }
    }
    let old = data.icon.replace(icon.0 as isize);
    if old != 0 {
        let _ = DestroyIcon(HICON(old as *mut _));
    }
    show(hwnd, IDC_ICON, !icon.is_invalid());
    set_text(
        hwnd,
        IDC_GLYPH,
        if icon.is_invalid() {
            outcome.glyph()
        } else {
            ""
        },
    );
    show(
        hwnd,
        IDC_GLYPH,
        icon.is_invalid() && outcome != Outcome::None,
    );
    invalidate_item(hwnd, IDC_GLYPH);
    invalidate_item(hwnd, IDC_ICON);
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

unsafe fn du(hwnd: HWND, x: i16, y: i16, w: i16, h: i16) -> RECT {
    let mut r = RECT {
        left: i32::from(x),
        top: i32::from(y),
        right: i32::from(x + w),
        bottom: i32::from(y + h),
    };
    let _ = MapDialogRect(hwnd, &mut r);
    r
}

unsafe fn place(hwnd: HWND, id: u16, x: i32, y: i32, w: i32, h: i32) {
    if let Ok(item) = GetDlgItem(Some(hwnd), i32::from(id)) {
        let _ = SetWindowPos(item, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

/// Runs `f` with a DC that has the control's own font selected.
unsafe fn with_font_dc<T>(hwnd: HWND, id: u16, f: impl FnOnce(HDC) -> T) -> Option<T> {
    let item = GetDlgItem(Some(hwnd), i32::from(id)).ok()?;
    let dc = GetDC(Some(item));
    let font = SendMessageW(item, WM_GETFONT, None, None).0;
    let old = (font != 0).then(|| SelectObject(dc, HGDIOBJ(font as *mut _)));
    let out = f(dc);
    if let Some(old) = old {
        SelectObject(dc, old);
    }
    ReleaseDC(Some(item), dc);
    Some(out)
}

unsafe fn text_width(hwnd: HWND, id: u16, text: &str) -> i32 {
    let wide: Vec<u16> = text.encode_utf16().collect();
    if wide.is_empty() {
        return 0;
    }
    with_font_dc(hwnd, id, |dc| {
        let mut size = SIZE::default();
        let _ = GetTextExtentPoint32W(dc, &wide, &mut size);
        size.cx
    })
    .unwrap_or(0)
}

unsafe fn wrapped_height(hwnd: HWND, id: u16, text: &str, width: i32) -> i32 {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    if wide.is_empty() {
        return 0;
    }
    with_font_dc(hwnd, id, |dc| {
        let mut r = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: 0,
        };
        DrawTextW(
            dc,
            &mut wide,
            &mut r,
            DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
        );
        r.bottom
    })
    .unwrap_or(0)
}

/// Positions everything from the content: controls are stacked top down in pixels, so none can overlap
/// and the details box is exactly as tall as its text (up to the space above the buttons).
struct Stack {
    left: i32,
    y: i32,
    w: i32,
    gap: i32,
}

impl Stack {
    unsafe fn put(&mut self, hwnd: HWND, id: u16, h: i32) {
        place(hwnd, id, self.left, self.y, self.w, h.max(1));
        self.y += h + self.gap;
    }
}

unsafe fn layout(
    hwnd: HWND,
    v: &View,
    has_icon: bool,
    details_shown: bool,
    link_rows: usize,
    cache_links: bool,
) {
    let line = du(hwnd, MARGIN, REPO_ROW, REPO_WIDTH, 12);
    let left = du(hwnd, MARGIN, 0, 0, 0).left;
    let full_w = du(hwnd, 0, 0, CONTENT_WIDTH, 0).right;
    let gap = du(hwnd, 0, 0, 0, 3).bottom;
    let x0 = line.left
        + if has_icon {
            du(hwnd, ICON_SLOT, 0, 0, 0).left
        } else {
            0
        };
    let name_w = (text_width(hwnd, IDC_REPO, &v.repo) + 6).min(line.right - x0);
    place(hwnd, IDC_REPO, x0, line.top, name_w, line.bottom - line.top);
    let x1 = x0 + name_w + gap;
    let suffix_w = (line.right - x1).max(0);
    let suffix = !v.repo_suffix.is_empty() && suffix_w > gap * 4;
    show(hwnd, IDC_REPO_SUFFIX, suffix);
    if suffix {
        place(
            hwnd,
            IDC_REPO_SUFFIX,
            x1,
            line.top,
            suffix_w,
            line.bottom - line.top,
        );
    }

    let y = du(hwnd, 0, VERDICT_ROW, 0, 0).top;
    let mut stack = Stack {
        left,
        y,
        w: full_w,
        gap,
    };
    let verdict_h = wrapped_height(hwnd, IDC_HEADLINE, &v.headline, full_w);
    place(hwnd, IDC_HEADLINE, left, stack.y, full_w, verdict_h.max(1));
    let mut top_h = verdict_h;
    if v.show_consent {
        let consent_h = wrapped_height(hwnd, IDC_CONSENT, &v.consent, full_w);
        place(hwnd, IDC_CONSENT, left, stack.y, full_w, consent_h.max(1));
        top_h = top_h.max(consent_h);
    }
    stack.y += top_h + gap;
    if v.running {
        stack.put(hwnd, IDC_PROGRESS, du(hwnd, 0, 0, 0, 8).bottom);
        stack.put(hwnd, IDC_PROGRESS_TEXT, du(hwnd, 0, 0, 0, 10).bottom);
    }
    for id in [IDC_LINKS, IDC_LINKS2].into_iter().take(link_rows) {
        stack.put(hwnd, id, du(hwnd, 0, 0, 0, 10).bottom);
    }
    if details_shown {
        let rate_h = du(hwnd, 0, 0, 0, 9).bottom;
        let cache_h = match &v.cache_line {
            Some(line) if cache_links => {
                wrapped_height(hwnd, IDC_CACHE, &strip_markup(&cache_markup(line)), full_w)
                    .max(rate_h)
            }
            _ => rate_h,
        };
        let limit = du(hwnd, 0, 164, 0, 0).top - gap - (cache_h - rate_h);
        let lines = GetDlgItem(Some(hwnd), i32::from(IDC_DETAILS))
            .map_or(1, |e| SendMessageW(e, EM_GETLINECOUNT, None, None).0 as i32);
        let text_h = with_font_dc(hwnd, IDC_DETAILS, |dc| {
            let mut tm = TEXTMETRICW::default();
            let _ = GetTextMetricsW(dc, &mut tm);
            tm.tmHeight + tm.tmExternalLeading
        })
        .unwrap_or(14);
        let h = (lines * text_h + 2 * gap).min(limit - stack.y).max(text_h);
        stack.put(hwnd, IDC_DETAILS, h);
        let buttons = du(hwnd, 0, 0, 0, 14).bottom;
        let sw = du(hwnd, 0, 0, 62, 0).right;
        place(hwnd, IDC_COPY_SHA, left, stack.y, sw, buttons);
        let sig_w = du(hwnd, 0, 0, 56, 0).right;
        place(
            hwnd,
            IDC_COPY_SIGNER,
            left + sw + gap,
            stack.y,
            sig_w,
            buttons,
        );
        stack.y += buttons + gap;
        place(hwnd, IDC_RATE, left, stack.y, full_w, rate_h);
        place(hwnd, IDC_CACHE, left, stack.y, full_w, cache_h);
    }
}

/// Changes whenever the layout may have moved, so stale pixels get erased.
fn shape_key(v: &View, has_icon: bool, link_rows: usize, cache_links: bool) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (
        has_icon,
        &v.repo,
        &v.repo_suffix,
        &v.repo_note,
        &v.headline,
        v.links.len(),
        (link_rows, cache_links),
        v.running,
        v.show_consent,
        v.rows.len(),
        v.notes.len(),
        v.rate_line.is_some(),
    )
        .hash(&mut h);
    h.finish()
}

/// Fills the link rows from the measured label widths, so a row never runs past the page edge
/// at any DPI or font; returns the number of rows used.
unsafe fn place_link_rows(hwnd: HWND, data: &PageData, links: &[Link]) -> usize {
    let rows = if data.has_link_class.get() && !links.is_empty() {
        let widths: Vec<i32> = links
            .iter()
            .map(|l| text_width(hwnd, IDC_LINKS, &l.label))
            .collect();
        let gap = text_width(hwnd, IDC_LINKS, "   ");
        let room = du(hwnd, 0, 0, CONTENT_WIDTH, 0).right - du(hwnd, 0, 0, 0, 3).bottom;
        split_rows(&widths, gap, room, 2)
    } else {
        Vec::new()
    };
    data.link_split.set(rows.first().map_or(0, |r| r.end));
    for (i, id) in [IDC_LINKS, IDC_LINKS2].into_iter().enumerate() {
        let text = rows
            .get(i)
            .map(|r| markup(&links[r.clone()]))
            .unwrap_or_default();
        set_text(hwnd, id, &text);
        show(hwnd, id, !text.is_empty());
    }
    rows.len()
}

unsafe fn render(hwnd: HWND, data: &PageData) {
    // Snapshot first: setting control text can notify us again while the cell is borrowed.
    let v: View = data.ui.borrow().view();
    data.tone.set(v.repo_tone);
    // The icon gets its own column only when there is one, so the text edge never shifts otherwise.
    let has_icon = v.outcome != Outcome::None;
    set_text(hwnd, IDC_REPO, &v.repo);
    set_text(hwnd, IDC_REPO_SUFFIX, &v.repo_suffix);
    set_text(hwnd, IDC_REPO_NOTE, &v.repo_note);
    if data.outcome.replace(v.outcome) != v.outcome {
        show_outcome(hwnd, data, v.outcome);
    }
    set_text(hwnd, IDC_HEADLINE, &v.headline);
    let link_rows = place_link_rows(hwnd, data, &v.links);
    let details = v.has_details();
    show(hwnd, IDC_DETAILS, details);
    set_text(hwnd, IDC_DETAILS, &v.details_text());
    if let Ok(edit) = GetDlgItem(Some(hwnd), i32::from(IDC_DETAILS)) {
        SendMessageW(edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(0)));
    }
    // Without the SysLink class the line is shown as plain text, without its actions.
    let cache_links = v.cache_line.is_some() && data.has_link_class.get();
    let rate_text = match (&v.rate_line, &v.cache_line) {
        (Some(rate), _) => rate.as_str(),
        (None, Some(cached)) if !cache_links => cached.as_str(),
        _ => "",
    };
    set_text(hwnd, IDC_RATE, rate_text);
    set_text(
        hwnd,
        IDC_CACHE,
        &v.cache_line
            .as_deref()
            .map(cache_markup)
            .unwrap_or_default(),
    );
    show(hwnd, IDC_CACHE, cache_links);
    show(hwnd, IDC_COPY_SHA, v.sha256.is_some());
    show(hwnd, IDC_COPY_SIGNER, v.signer.is_some());
    set_text(hwnd, IDC_CONSENT, &v.consent);
    show(hwnd, IDC_CONSENT, v.show_consent);
    set_text(hwnd, IDC_VERIFY, &v.verify_label);
    set_text(hwnd, IDC_PROGRESS_TEXT, &v.progress_text);
    layout(hwnd, &v, has_icon, details, link_rows, cache_links);
    let shape = shape_key(&v, has_icon, link_rows, cache_links);
    if data.shape.replace(shape) != shape {
        let _ = RedrawWindow(
            Some(hwnd),
            None,
            None,
            RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_UPDATENOW,
        );
    }
    show(hwnd, IDC_PROGRESS, v.running);
    show(hwnd, IDC_CANCEL, v.running);
    show(hwnd, IDC_VERIFY, !v.running);
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
        ui.tz = jiff::tz::TimeZone::system();
    }
    create_fonts(hwnd, data);
    create_text(hwnd, IDC_LINKS, LINKS_RECT, "", data);
    create_text(hwnd, IDC_LINKS2, LINKS_RECT, "", data);
    create_text(hwnd, IDC_CACHE, CACHE_RECT, "", data);
    set_font(hwnd, IDC_CACHE, data.fonts.get()[3]);
    create_text(hwnd, IDC_FOOTER, FOOTER_RECT, FOOTER_MARKUP, data);
    add_tooltip(hwnd);
    #[cfg(feature = "demo-report")]
    inject_demo_report(data);
    render(hwnd, data);
}

#[cfg(feature = "demo-report")]
fn inject_demo_report(data: &PageData) {
    let Some(path) = std::env::var_os("SIGSTORE_SHELL_DEMO_REPORT") else {
        return;
    };
    let report = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    if let Some(report) = report {
        data.mailbox.post(Event::Start { job: 0 });
        data.mailbox.post(Event::Finished {
            job: 0,
            result: Ok(Box::new(report)),
        });
    }
}

/// Explains the counter in the Verify label; the text is static, so the tooltip may keep the pointer.
unsafe fn add_tooltip(hwnd: HWND) {
    let Ok(button) = GetDlgItem(Some(hwnd), i32::from(IDC_VERIFY)) else {
        return;
    };
    let Ok(tip) = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        w!("tooltips_class32"),
        PCWSTR::null(),
        WINDOW_STYLE(WS_POPUP.0 | TTS_ALWAYSTIP),
        0,
        0,
        0,
        0,
        Some(hwnd),
        None,
        Some(HINSTANCE(module().0)),
        None,
    ) else {
        return;
    };
    let info = TTTOOLINFOW {
        cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
        uFlags: TTF_IDISHWND | TTF_SUBCLASS,
        hwnd,
        uId: button.0 as usize,
        lpszText: PWSTR(
            w!("GitHub API requests left this hour for this IP; shared with other tools on this network.")
                .as_ptr()
                .cast_mut(),
        ),
        ..Default::default()
    };
    SendMessageW(tip, TTM_SETMAXTIPWIDTH, None, Some(LPARAM(300)));
    SendMessageW(
        tip,
        TTM_ADDTOOLW,
        None,
        Some(LPARAM(&info as *const TTTOOLINFOW as isize)),
    );
}

unsafe fn on_copy(hwnd: HWND, data: &PageData, id: u16) {
    let text = {
        let v = data.ui.borrow().view();
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
    if (*nm).idFrom == usize::from(IDC_CACHE) {
        let shown = data.ui.borrow().view().cache_line.is_some();
        match index {
            0 if shown => on_verify(hwnd, data, true),
            1 if shown => on_clear(hwnd, data),
            _ => {}
        }
        return;
    }
    let url = match (*nm).idFrom {
        id if id == usize::from(IDC_FOOTER) => FOOTER_URLS.get(index).map(|u| (*u).to_string()),
        id if id == usize::from(IDC_LINKS) || id == usize::from(IDC_LINKS2) => {
            let first = if id == usize::from(IDC_LINKS) {
                0
            } else {
                data.link_split.get()
            };
            let v = data.ui.borrow().view();
            v.links.get(first + index).map(|l| l.url.clone())
        }
        _ => None,
    };
    if let Some(url) = url {
        open_url(hwnd, &url);
    }
}

/// Statics drawn over the themed page background; the colour is None to keep the default.
unsafe fn static_color(id: u16, data: &PageData) -> Option<Option<COLORREF>> {
    match id {
        IDC_GLYPH => Some(Some(outcome_color(data.outcome.get()))),
        IDC_REPO if data.tone.get() == Tone::Bad => Some(Some(outcome_color(Outcome::Bad))),
        IDC_REPO => Some(Some(COLORREF(GetSysColor(COLOR_WINDOWTEXT)))),
        IDC_EXPLAIN | IDC_CONSENT | IDC_RATE => Some(Some(outcome_color(Outcome::Neutral))),
        _ => None,
    }
}

unsafe fn themed_static(ctl: HWND, hdc: HDC, color: Option<COLORREF>) -> isize {
    let _ = DrawThemeParentBackground(ctl, hdc, None);
    if let Some(c) = color {
        SetTextColor(hdc, c);
    }
    SetBkMode(hdc, TRANSPARENT);
    GetStockObject(NULL_BRUSH).0 as isize
}

/// `refresh` skips the cache so GitHub is asked now.
unsafe fn on_verify(hwnd: HWND, data: &PageData, refresh: bool) {
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
            refresh,
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

/// Deletes this file's cached entry only; the result on screen stays, relabelled as no longer cached.
unsafe fn on_clear(hwnd: HWND, data: &PageData) {
    {
        let mut ui = data.ui.borrow_mut();
        let State::Done(report) = &ui.state else {
            return;
        };
        let cleared = Cache::user().is_some_and(|c| c.remove(&report.file_sha256).is_ok());
        if cleared {
            ui.state = std::mem::replace(&mut ui.state, State::Idle).apply(Event::CacheCleared);
        }
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
            if let Event::Finished {
                result: Ok(report), ..
            } = &event
            {
                if let Some(rate) = Rate::from_report(report) {
                    ui.rate = Some(rate);
                }
            }
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
                        IDC_VERIFY => on_verify(hwnd, data, false),
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
            WM_DRAWITEM => {
                let item = (lparam.0 as *const DRAWITEMSTRUCT).as_ref();
                if let (Some(data), Some(item)) = (data_of(hwnd), item) {
                    if item.CtlID == u32::from(IDC_ICON) {
                        draw_icon(data, item);
                        return 1;
                    }
                }
            }
            WM_CTLCOLORSTATIC => {
                let ctl = HWND(lparam.0 as *mut _);
                if let Some(data) = data_of(hwnd) {
                    let id = u16::try_from(GetDlgCtrlID(ctl)).unwrap_or(0);
                    if let Some(color) = static_color(id, data) {
                        return themed_static(ctl, HDC(wparam.0 as *mut _), color);
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
                    let icon = data.icon.replace(0);
                    if icon != 0 {
                        let _ = DestroyIcon(HICON(icon as *mut _));
                    }
                }
                SetWindowLongPtrW(hwnd, DWLP_USER, 0);
            }
            _ => {}
        }
        0
    })
}
