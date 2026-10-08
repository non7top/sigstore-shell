use crate::cache::Cache;
use crate::job::{self, JobError, JobInput};
use crate::model::{Event, Phase};
use crate::page::Mailbox;
use provenance_core::{
    load_trusted_root, load_trusted_root_offline, GithubProvider, Provider, RekorProvider,
};
use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use windows::core::{Result, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, FreeLibrary, HMODULE};
use windows::Win32::System::LibraryLoader::{
    FreeLibraryAndExitThread, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
};
use windows::Win32::System::Threading::{CreateThread, THREAD_CREATION_FLAGS};

pub struct JobCtx {
    pub job: u64,
    pub path: PathBuf,
    pub claimed_repo: Option<String>,
    pub rekor: bool,
    /// Skip the cache and ask GitHub now.
    pub refresh: bool,
    pub cancel: Arc<AtomicBool>,
    pub mailbox: Arc<Mailbox>,
}

fn run_job(ctx: &JobCtx) {
    let outcome = (|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| JobError::Failed(format!("could not start the network runtime: {e}")))?;
        let mut providers: Vec<Box<dyn Provider>> = vec![Box::new(GithubProvider::new(None))];
        if ctx.rekor {
            providers.push(Box::new(RekorProvider::default()));
        }
        let cache = Cache::user();
        let last = AtomicU8::new(u8::MAX);
        let progress = |phase: Phase| {
            if let Phase::Hashing { percent } = phase {
                if last.swap(percent, Ordering::Relaxed) == percent {
                    return;
                }
            }
            ctx.mailbox.post(Event::Progress {
                job: ctx.job,
                phase,
            });
        };
        let input = JobInput {
            path: &ctx.path,
            claimed_repo: ctx.claimed_repo.clone(),
            refresh: ctx.refresh,
        };
        runtime.block_on(job::run(
            &input,
            cache.as_ref(),
            &providers,
            |offline| async move {
                if offline {
                    load_trusted_root_offline().await
                } else {
                    load_trusted_root().await
                }
            },
            &ctx.cancel,
            &progress,
        ))
    })();
    match outcome {
        // The page already moved on when the user cancelled.
        Err(JobError::Cancelled) => {}
        Err(JobError::Failed(e)) => ctx.mailbox.post(Event::Finished {
            job: ctx.job,
            result: Err(e),
        }),
        Ok(report) => ctx.mailbox.post(Event::Finished {
            job: ctx.job,
            result: Ok(Box::new(report)),
        }),
    }
}

unsafe extern "system" fn thread_main(param: *mut c_void) -> u32 {
    let (ctx, module) = *Box::from_raw(param.cast::<(JobCtx, HMODULE)>());
    let panicked = catch_unwind(AssertUnwindSafe(|| run_job(&ctx))).is_err();
    if panicked {
        ctx.mailbox.post(Event::Finished {
            job: ctx.job,
            result: Err("internal error in the verification thread".into()),
        });
    }
    drop(ctx);
    FreeLibraryAndExitThread(module, 0)
}

/// The thread holds a reference on this DLL so Explorer cannot unload it mid-run.
pub fn spawn(ctx: JobCtx) -> Result<()> {
    unsafe {
        let mut module = HMODULE::default();
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
            PCWSTR(thread_main as *const u16),
            &mut module,
        )?;
        let param = Box::into_raw(Box::new((ctx, module)));
        match CreateThread(
            None,
            0,
            Some(thread_main),
            Some(param.cast_const().cast()),
            THREAD_CREATION_FLAGS(0),
            None,
        ) {
            Ok(handle) => {
                let _ = CloseHandle(handle);
                Ok(())
            }
            Err(e) => {
                drop(Box::from_raw(param));
                let _ = FreeLibrary(module);
                Err(e)
            }
        }
    }
}
