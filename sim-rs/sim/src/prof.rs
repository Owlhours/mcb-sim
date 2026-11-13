//! A tiny sampling profiler (no tools are installed here): MCSIM_PROF=<file> samples the program counter on a CPU
//! timer and writes the offsets into the executable; `tools/prof.py` turns them into a per-function table.
use std::sync::atomic::{AtomicUsize, Ordering};

const MAX: usize = 1 << 20;
static mut SAMPLES: [usize; MAX] = [0; MAX];
static N: AtomicUsize = AtomicUsize::new(0);

#[repr(C)]
struct Timeval { sec: i64, usec: i64 }
#[repr(C)]
struct Itimerval { interval: Timeval, value: Timeval }
#[repr(C)]
struct Sigaction { handler: usize, flags: u64, restorer: usize, mask: [u64; 16] }

extern "C" {
    fn setitimer(which: i32, new: *const Itimerval, old: *mut Itimerval) -> i32;
    fn sigaction(sig: i32, act: *const Sigaction, old: *mut Sigaction) -> i32;
}

extern "C" fn on_prof(_sig: i32, _info: *const u8, ctx: *const u8) {
    // x86-64 Linux: ucontext_t.uc_mcontext.gregs[REG_RIP] is at byte offset 168
    let rip = unsafe { *(ctx.add(168) as *const usize) };
    let k = N.fetch_add(1, Ordering::Relaxed);
    if k < MAX { unsafe { SAMPLES[k] = rip; } }
}

pub fn start() {
    let act = Sigaction { handler: on_prof as *const () as usize, flags: 4 | 0x10000000, restorer: 0, mask: [0; 16] }; // SA_SIGINFO | SA_RESTART
    let tv = Itimerval { interval: Timeval { sec: 0, usec: 500 }, value: Timeval { sec: 0, usec: 500 } };
    unsafe {
        sigaction(27, &act, std::ptr::null_mut()); // SIGPROF
        setitimer(2, &tv, std::ptr::null_mut()); // ITIMER_PROF
    }
}

pub fn finish(path: &str) {
    let zero = Itimerval { interval: Timeval { sec: 0, usec: 0 }, value: Timeval { sec: 0, usec: 0 } };
    unsafe { setitimer(2, &zero, std::ptr::null_mut()); }
    // load address of the executable: first mapping in /proc/self/maps
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
    let base = maps.lines().next().and_then(|l| l.split('-').next()).and_then(|a| usize::from_str_radix(a, 16).ok()).unwrap_or(0);
    let n = N.load(Ordering::Relaxed).min(MAX);
    let mut out = String::with_capacity(n * 10);
    for k in 0..n { let a = unsafe { SAMPLES[k] }; out.push_str(&format!("{:x}\n", a.wrapping_sub(base))); }
    let _ = std::fs::write(path, out);
}
