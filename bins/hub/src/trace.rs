//! `--trace`: stage times, memory and CPU time on stderr, every line starting `hub:`.
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub struct Trace {
    pub on: bool,
    pub started: Instant,
}

fn mib(b: u64) -> f64 {
    b as f64 / (1024.0 * 1024.0)
}

impl Trace {
    pub fn new(on: bool, started: Instant) -> Trace {
        Trace { on, started }
    }

    pub fn since(&self) -> f64 {
        self.started.elapsed().as_secs_f64() * 1000.0
    }

    pub fn stage(&self, what: &str) {
        if self.on {
            eprintln!("hub: {:7.1} ms  {what}", self.since());
        }
    }

    pub fn line(&self, text: &str) {
        if self.on {
            eprintln!("hub: {text}");
        }
    }

    pub fn memory(&self, when: &str) {
        if !self.on {
            return;
        }
        match memory() {
            Some((private, peak)) => eprintln!(
                "hub: memory {when}: private {:.1} MiB, peak working set {:.1} MiB",
                mib(private),
                mib(peak)
            ),
            None => eprintln!("hub: memory {when}: not available"),
        }
    }

    pub fn cpu(&self, when: &str) {
        if !self.on {
            return;
        }
        match cpu_time() {
            Some(d) => eprintln!(
                "hub: cpu time {when}: {:.1} ms over {:.0} ms",
                d.as_secs_f64() * 1000.0,
                self.since()
            ),
            None => eprintln!("hub: cpu time {when}: not available"),
        }
    }
}

/// (private bytes, peak working set).
#[cfg(windows)]
pub fn memory() -> Option<(u64, u64)> {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: a zeroed counters struct is valid; the call fills at most `cb` bytes of it.
    let mut c: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
    c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    // SAFETY: `c` is writable and correctly sized; its layout starts with PROCESS_MEMORY_COUNTERS;
    // the pseudo handle needs no closing.
    let ok = unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut c as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
            c.cb,
        )
    };
    (ok != 0).then_some((c.PrivateUsage as u64, c.PeakWorkingSetSize as u64))
}

#[cfg(not(windows))]
pub fn memory() -> Option<(u64, u64)> {
    None
}

/// CPU time the process has used, kernel and user together.
#[cfg(windows)]
pub fn cpu_time() -> Option<Duration> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: four writable FILETIMEs and the current-process pseudo handle.
    let ok = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    };
    let t = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
    (ok != 0).then(|| Duration::from_nanos((t(kernel) + t(user)) * 100))
}

#[cfg(not(windows))]
pub fn cpu_time() -> Option<Duration> {
    None
}
