//! Native benchmark counters describe CPU work, never wall-clock time. Windows
//! and POSIX expose the same thread/process totals through different APIs.
#[cfg(unix)]
pub fn thread_cpu() -> f64 {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t);
    }
    t.tv_sec as f64 * 1e3 + t.tv_nsec as f64 / 1e6
}
#[cfg(unix)]
pub fn process_cpu() -> f64 {
    let mut u: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe {
        libc::getrusage(libc::RUSAGE_SELF, &mut u);
    }
    let ms = |t: libc::timeval| t.tv_sec as f64 * 1e3 + t.tv_usec as f64 / 1e3;
    ms(u.ru_utime) + ms(u.ru_stime)
}
#[cfg(windows)]
fn windows_cpu(thread: bool) -> f64 {
    use windows_sys::Win32::{
        Foundation::FILETIME,
        System::Threading::{GetCurrentProcess, GetCurrentThread, GetProcessTimes, GetThreadTimes},
    };
    let mut created = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut exited = created;
    let mut kernel = created;
    let mut user = created;
    // SAFETY: the pseudo handle belongs to this process/thread. Every output
    // points to a distinct live FILETIME until the synchronous call completes.
    let success = unsafe {
        if thread {
            GetThreadTimes(
                GetCurrentThread(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        } else {
            GetProcessTimes(
                GetCurrentProcess(),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        }
    };
    assert!(
        success != 0,
        "native CPU counter failed: {}",
        std::io::Error::last_os_error()
    );
    // FILETIME stores 100 ns units. Include both words before conversion so a
    // long render never wraps at the low word's roughly seven-minute boundary.
    let ms = |t: FILETIME| {
        ((u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)) as f64 / 10000.0
    };
    ms(kernel) + ms(user)
}
#[cfg(windows)]
pub fn thread_cpu() -> f64 {
    windows_cpu(true)
}
#[cfg(windows)]
pub fn process_cpu() -> f64 {
    windows_cpu(false)
}
#[cfg(test)]
mod tests {
    #[test]
    fn counters_are_monotonic_and_measure_current_process_work() {
        let (thread, process) = (super::thread_cpu(), super::process_cpu());
        let mut n = 1u64;
        for i in 0..100_000 {
            n = n.wrapping_mul(1664525).wrapping_add(i);
            std::hint::black_box(n);
        }
        assert!(super::thread_cpu() >= thread);
        assert!(super::process_cpu() >= process);
    }
}
