#[cfg(unix)]
use std::fs::{File, OpenOptions};
#[cfg(unix)]
use std::io;

#[cfg(unix)]
use anyhow::Context;
use anyhow::{Result, anyhow};

pub struct SingleInstanceGuard {
    #[cfg(unix)]
    _file: File,
    #[cfg(windows)]
    acquired: bool,
}

pub fn acquire_single_instance() -> Result<SingleInstanceGuard> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;

        const LOCK_EX: i32 = 2;
        const LOCK_NB: i32 = 4;
        unsafe extern "C" {
            fn flock(fd: i32, operation: i32) -> i32;
        }

        let path = crate::ipc_endpoint().with_extension("lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("open engine lock {}", path.display()))?;
        if unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) } != 0 {
            let error = io::Error::last_os_error();
            return Err(anyhow!(
                "Virtual MIDI Master engine is already running: {error}"
            ));
        }
        return Ok(SingleInstanceGuard { _file: file });
    }

    #[cfg(windows)]
    {
        unsafe extern "C" {
            fn vmm_instance_lock_acquire() -> i32;
        }
        if unsafe { vmm_instance_lock_acquire() } == 0 {
            return Err(anyhow!("Virtual MIDI Master engine is already running"));
        }
        return Ok(SingleInstanceGuard { acquired: true });
    }

    #[allow(unreachable_code)]
    Err(anyhow!("single-instance locking is unsupported"))
}

#[cfg(windows)]
impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        unsafe extern "C" {
            fn vmm_instance_lock_release();
        }
        if self.acquired {
            unsafe { vmm_instance_lock_release() };
            self.acquired = false;
        }
    }
}
