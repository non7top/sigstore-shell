use crate::dlgtemplate::{
    build, IDC_CANCEL, IDC_CLAIM, IDC_CONSENT, IDC_DETAILS, IDC_HEADLINE, IDC_PROGRESS,
    IDC_PROGRESS_TEXT, IDC_VERIFY,
};
use crate::dll::{guard_value, module, Live};
use crate::model::{view, Claim, Event, State, View};
use crate::settings::{app_dir, Settings};
use crate::worker::{self, JobCtx};
use provenance_core::read_claim_file;
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use windows::core::{w, Result};
use windows::Win32::Foundation::{E_FAIL, HINSTANCE, HWND, LPARAM, WPARAM};
use windows::Win32::UI::Controls::{
    CreatePropertySheetPageW, HPROPSHEETPAGE, PBM_SETMARQUEE, PROPSHEETPAGEW, PROPSHEETPAGEW_0,
    PSPCB_MESSAGE, PSPCB_RELEASE, PSP_DLGINDIRECT, PSP_USECALLBACK, PSP_USETITLE,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    GetDlgItem, GetWindowLongPtrW, PostMessageW, SendMessageW, SetWindowLongPtrW, SetWindowTextW,
    ShowWindow, DLGTEMPLATE, SW_HIDE, SW_SHOW, WINDOW_LONG_PTR_INDEX, WM_APP, WM_COMMAND,
    WM_DESTROY, WM_INITDIALOG,
};

/// DWLP_USER in winuser.h (DWLP_DLGPROC + pointer size); the windows crate only has the 32-bit DWL_USER.
const DWLP_USER: WINDOW_LONG_PTR_INDEX = WINDOW_LONG_PTR_INDEX(16);

const WM_APP_UPDATE: u32 = WM_APP + 1;
const BN_CLICKED: u32 = 0;

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

unsafe fn render(hwnd: HWND, data: &PageData) {
    // Snapshot first: setting control text can notify us again while the cell is borrowed.
    let v: View = {
        let ui = data.ui.borrow();
        view(&ui.claim, &ui.state, ui.rekor)
    };
    set_text(hwnd, IDC_CLAIM, &v.claim_line);
    set_text(hwnd, IDC_HEADLINE, &v.headline);
    set_text(hwnd, IDC_DETAILS, &v.details_text());
    set_text(hwnd, IDC_CONSENT, &v.consent);
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
    if let Ok(verify) = GetDlgItem(Some(hwnd), i32::from(IDC_VERIFY)) {
        let _ = EnableWindow(verify, v.can_verify);
    }
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
    render(hwnd, data);
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
                return 1;
            }
            WM_COMMAND => {
                let id = (wparam.0 & 0xFFFF) as u16;
                let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
                if let (Some(data), BN_CLICKED) = (data_of(hwnd), code) {
                    match id {
                        IDC_VERIFY => on_verify(hwnd, data),
                        IDC_CANCEL => on_cancel(hwnd, data),
                        _ => {}
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
                }
                SetWindowLongPtrW(hwnd, DWLP_USER, 0);
            }
            _ => {}
        }
        0
    })
}
