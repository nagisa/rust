// Copyright 2014-2015 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.
//! Rust stack management
//!
//! Rust guard page scheme in order to detect stack overflow for rust threads. Sadly, the stack
//! probes are not supported by LLVM on non-Windows platform. This means that it should be fairly
//! easy to trigger a “stack overflow” by defining and using a function with huge stack space
//! requirements.
//!
//! This module implements the functionality necessary to introspect, setup and manage stacks of
//! native threads on UNIXy systems.
//!
//! This function is an unstable module because this scheme for stack overflow detection is not
//! guaranteed to continue in the future. Usage of this module is discouraged unless absolutely
//! necessary.
#![allow(dead_code)] // some platforms may not use some of the functions...
#![allow(unused_imports)] // or imports

use cell::RefCell;
use core::prelude::*;
use libc;
use mem;
use env;

pub use self::imp::setup;

pub const RED_ZONE: usize = 0x5000;

// SNAP: please inline the function body into its callers
#[cfg(not(stage0))]
#[inline(always)]
unsafe fn frame_address(f: u32) -> *const u8 {
    use intrinsics;
    intrinsics::frame_address(f)
}

#[cfg(stage0)]
#[inline(always)]
unsafe fn frame_address(_: u32) -> *const u8 {
    let val = 0;
    &val as *const _
}

thread_local!(static THREAD_STACK: RefCell<Option<imp::Stack>> = RefCell::new(None));

#[cfg(all(not(target_os = "linux"),
          not(target_os = "macos"),
          not(target_os = "bitrig"),
          not(target_os = "openbsd"),
          not(target_os = "freebsd")))]
mod imp {
    type Stack = ();

    /// Setup the stack information for a thread.
    ///
    /// Must be called from the thread that is being set up. The earlier in the stack this function
    /// is called the better. Calling the function multiple times for the same thread is undefined.
    #[inline(always)]
    pub unsafe fn setup(_is_main: bool) {
    }
}

#[cfg(any(target_os = "linux",
          target_os = "macos",
          target_os = "bitrig",
          target_os = "openbsd",
          target_os = "freebsd"))]
mod imp {
    use core::prelude::*;

    use ptr;
    use mem;
    use libc;
    use intrinsics;
    use super::signal::*;
    use super::{RED_ZONE, THREAD_STACK, sigaltstack, sigaction, signal, raise, record_sp_limit,
                main_stack_extents, pthread_stack_extents};
    use env;
    use libc::{mmap, munmap};
    use libc::{PROT_NONE, PROT_READ, PROT_WRITE, MAP_PRIVATE, MAP_ANON, MAP_FAILED, MAP_FIXED,
               SIGSEGV};

    pub struct Stack {
        /// Stack top. Value represents an address which may point anywhere on the stack guard.
        pub top: usize,
        /// Stack bottom. Value represents an address which will always point inside the
        /// stack, but not necessarily at the highest addressable point of the stack.
        pub bottom: usize,
        /// Pointer to the top of alternative signal handler stack.
        pub handler: usize
    }

    impl Drop for Stack {
        fn drop(&mut self) {unsafe {
            // It is crucial this function is only called when the thread’s lifetime ends, which
            // thread_local! provides for us at the moment.
            //
            // We don’t need to unmap guard, because it is inside the stack and gets unmapped
            // together with it. Unmap the alternate signal handler stack only.
            let result = munmap(self.handler as *mut _, SIGSTKSZ);
            assert_eq!(result, 0);
        }}
    }

    #[inline(always)]
    pub unsafe fn setup(is_main: bool) {
        if !is_main {
            // The thread has been created by pthread. pthread stores all its attributes somewhere
            // on the thread stack, and we can simply ask it to read it for us.
            let (top, bottom) = pthread_stack_extents();
            let new_top = init(top, bottom, true);
            // This can be simply removed once stack probing gets implemented
            record_sp_limit(new_top + RED_ZONE);
        } else {
            // None of that is applicable to main thread, though. It usually has envp, argc and
            // argv data at the end of the stack and pthread will sometimes do some reading from
            // the filesystem in order to find out the precise stack bounds. Depending on OS we
            // have to calculate bounds ourselves less accurately instead.
            let (top, bottom) = main_stack_extents();
            // We don’t need to allocate a guard, because OS has already set up one for us.
            let new_top = init(top, bottom, false);
            // This can be simply removed once stack probing gets implemented
            record_sp_limit(new_top + RED_ZONE);
        }
    }

    #[inline(always)]
    unsafe fn init(top: usize, bottom: usize, with_guard: bool) -> usize {
        // Override SIGSEGV and SIGBUS action so we can react to stack overflows.
        let mut action: sigaction = mem::zeroed();
        action.sa_flags = SA_SIGINFO | SA_ONSTACK;
        action.sa_sigaction = signal_handler as sighandler_t;
        assert_eq!(sigaction(SIGSEGV, &action, ptr::null_mut()), 0);
        assert_eq!(sigaction(SIGBUS, &action, ptr::null_mut()), 0);
        // Allocate an alternative stack for thread’s signal handling. Note, that the order for
        // registering action and setting the stack is important.
        let alt_stack = mmap(ptr::null_mut(),
                             SIGSTKSZ,
                             PROT_READ | PROT_WRITE,
                             MAP_PRIVATE | MAP_ANON,
                             -1, 0);
        assert!(alt_stack != MAP_FAILED);
        let signal_stack = sigaltstack {
            ss_sp: alt_stack,
            ss_flags: 0,
            ss_size: SIGSTKSZ
        };
        sigaltstack(&signal_stack, ptr::null_mut());

        let new_top = if with_guard {
            allocate_guard(top)
        } else {
            top
        };

        THREAD_STACK.with(|stack_ref| {
            *stack_ref.borrow_mut() = Some(Stack {
                top: top,
                bottom: bottom,
                handler: alt_stack as usize
            });
        });
        new_top
    }

    /// Allocate a guard at the `top` and return address of the new top of stack.
    unsafe fn allocate_guard(top: usize) -> usize {
        let page_size = env::page_size();
        // Ensure top address is page aligned! There’s many ways this could happen:
        // * Thread created with non-page-aligned stack size;
        // * RLIMIT_STACK set to non-page-aligned size;
        // * and so on.
        let misalignment = top % page_size;
        let top = if misalignment != 0 {
            // Since this is the address of the top, we must align towards the higher addresses
            top + misalignment
        } else {
            top
        };
        // mmap a page on the top of the stack.
        // This insures a SIGBUS or SIGSEGV will be raised on stack overflow.
        let new_top = mmap(top as *mut _,
                           page_size as libc::size_t,
                           PROT_NONE,
                           MAP_PRIVATE | MAP_ANON | MAP_FIXED,
                           -1, 0);
        assert!(new_top != MAP_FAILED || new_top as usize == top);
        new_top as usize + page_size
    }

    #[no_stack_check]
    pub unsafe extern fn signal_handler(signum: libc::c_int,
                                             info: *mut siginfo,
                                             _data: *mut libc::c_void) {
        // This should already be set to 0, but in case it is not when this function is called, we
        // would be sad.
        record_sp_limit(0);
        // We can not return from a SIGSEGV or SIGBUS signal.
        // See: https://www.gnu.org/software/libc/manual/html_node/Handler-Returns.html
        unsafe fn term(signum: libc::c_int) -> ! {
            use core::mem::transmute;
            signal(signum, transmute(signal::SIG_DFL));
            raise(signum);
            intrinsics::abort();
        }
        let (top, bottom) = THREAD_STACK.with(|stack_ref| {
            if let Some(ref stack) = *stack_ref.borrow() {
                (stack.top, stack.bottom)
            } else {
                term(signum)
            }
        });
        let addr = (*info).si_addr as usize;
        // If the fault address falls onto the stack, we can assume it is caused by stack overflow.
        // The only other reason is user mapping a guard page of his own somewhere on the stack.
        if addr < top  || addr >= bottom {
            term(signum);
        }
        ::rt::util::report_overflow();
        intrinsics::abort()
    }
}

/// This function is invoked from the __morestack function.
#[cfg(not(test))] // in testing, use the original libstd's version
#[lang = "stack_exhausted"]
extern fn stack_exhausted() {
    use intrinsics;
    unsafe {
        // Since we call functions with stack checks, remove the limit.
        record_sp_limit(0);
        // And fail after printing a nice message…
        ::rt::util::report_overflow();
        intrinsics::abort();
    }
}

#[cfg(all(not(target_os = "macos"),
          not(target_os = "openbsd"),
          not(target_os = "bitrig"),
          not(target_os = "freebsd")))]
#[inline(always)]
pub unsafe fn main_stack_extents() -> (usize, usize) {
    // Portable version to get main stack extents somewhat reliably by only querying OS for
    // "trivial" information.
    let page_size = env::page_size();
    let bottom = frame_address(0) as usize;
    debug_assert!(bottom != 0);
    let mut resource = mem::zeroed();
    assert_eq!(libc::getrlimit(libc::RLIMIT_STACK, &mut resource as *mut _), 0);
    let max_stack = resource.rlim_cur as usize;

    // Main stack may not be misaligned
    let misalignment = bottom % page_size;
    let bottom = if misalignment != 0 {
        bottom - misalignment
    } else {
        bottom
    };
    // `bottom - max_stack` is guaranteed to be an address inside or even a little bit past the OS
    // allocated guard page. Both these scenarios are fine, because for the main thread this
    // information is only used to check whether the fault happened on the guard.
    (bottom.checked_sub(max_stack).unwrap_or(page_size), bottom)
}

#[cfg(any(target_os = "macos",
          target_os = "openbsd",
          target_os = "bitrig",
          target_os = "freebsd"))]
#[inline(always)]
pub unsafe fn main_stack_extents() -> (usize, usize) {
    let extents = pthread_stack_extents();
    let page_size = env::page_size();
    // Subtract one page form the stack top address because pthread will make sure to return lowest
    // address that is addressable and is not a guard page. Our code, on the other hand, expects
    // the top pointer to be somewhere in the guard.
    (extents.0.checked_sub(page_size).unwrap_or(page_size), extents.1)
}

#[cfg(any(target_os = "linux",
          target_os = "android"))]
#[inline(always)]
pub unsafe fn pthread_stack_extents() -> (usize, usize) {
    use ptr;

    let mut attr: libc::pthread_attr_t = mem::zeroed();
    assert_eq!(pthread_getattr_np(pthread_self(), &mut attr), 0);
    let mut stacktop = ptr::null_mut();
    let mut stacksize = 0;
    assert_eq!(pthread_attr_getstack(&attr, &mut stacktop, &mut stacksize), 0);
    assert_eq!(pthread_attr_destroy(&mut attr), 0);
    (stacktop as usize, stacktop as usize + (stacksize as usize))
}

#[cfg(target_os = "freebsd")]
#[inline(always)]
pub unsafe fn pthread_stack_extents() -> (usize, usize) {
    use ptr;

    let mut attr: libc::pthread_attr_t = mem::zeroed();
    assert_eq!(pthread_attr_init(&mut attr), 0);
    assert_eq!(pthread_attr_get_np(pthread_self(), &mut attr), 0);
    let mut stacktop = ptr::null_mut();
    let mut stacksize = 0;
    assert_eq!(pthread_attr_getstack(&attr, &mut stacktop, &mut stacksize), 0);
    assert_eq!(pthread_attr_destroy(&mut attr), 0);
    (stacktop as usize, stacktop as usize + (stacksize as usize))
}

#[cfg(target_os = "macos")]
#[inline(always)]
pub unsafe fn pthread_stack_extents() -> (usize, usize) {
    let stackbottom = pthread_get_stackaddr_np(pthread_self()) as usize;
    let stacksize = pthread_get_stacksize_np(pthread_self()) as usize;
    (stackbottom - stacksize, stackbottom)
}

#[cfg(any(target_os = "openbsd", target_os = "bitrig"))]
#[inline(always)]
pub unsafe fn pthread_stack_extents() -> (usize, usize) {
    let mut current_stack: stack_t = mem::zeroed();
    assert_eq!(pthread_stackseg_np(pthread_self(), &mut current_stack), 0);

    let extra = if cfg!(target_os = "bitrig") {3} else {1} * os::page_size();
    let stackbottom = current_stack.ss_sp as usize;
    let stacksize = if pthread_main_np() == 1 {
        current_stack.ss_size - extra
    } else {
        current_stack.ss_size
    };
    (stackbottom - stacksize, stackbottom)
}

/// Records the current limit of the stack as specified by `limit`.
///
/// This is stored in an OS-dependent location, likely inside of the thread
/// local storage. The location that the limit is stored is a pre-ordained
/// location because it's where LLVM has emitted code to check.
///
/// Note that this cannot be called under normal circumstances. This function is
/// changing the stack limit, so upon returning any further function calls will
/// possibly be triggering the morestack logic if you're not careful.
///
/// Also note that these functions are all flagged as "inline(always)" because they're messing
/// around with the stack limits. This would be unfortunate for the functions themselves to
/// trigger a morestack invocation (if they were an actual function call).
///
/// Finally, these functions are only used by __morestack and we don’t actually bother telling them
/// any other values other than 0.
#[cfg(all(target_arch = "x86_64",
          any(target_os = "macos",
              target_os = "ios")))]
#[inline(always)]
unsafe fn record_sp_limit(limit: usize) {
    asm!("movq $$0x60+90*8, %rsi
          movq $0, %gs:(%rsi)" :: "r"(limit) : "rsi" : "volatile")
}

#[cfg(all(target_arch = "x86_64",
          target_os = "linux"))]
#[inline(always)]
pub unsafe fn record_sp_limit(limit: usize) {
    asm!("movq $0, %fs:112" :: "r"(limit) :: "volatile")
}

#[cfg(all(target_arch = "x86_64",
          target_os = "freebsd"))]
#[inline(always)]
pub unsafe fn record_sp_limit(limit: usize) {
    asm!("movq $0, %fs:24" :: "r"(limit) :: "volatile")
}

#[cfg(all(target_arch = "x86_64",
          target_os = "dragonfly"))]
#[inline(always)]
pub unsafe fn record_sp_limit(limit: usize) {
    asm!("movq $0, %fs:32" :: "r"(limit) :: "volatile")
}

#[cfg(all(target_arch = "x86",
          any(target_os = "macos",
              target_os = "ios")))]
#[inline(always)]
pub unsafe fn record_sp_limit(limit: usize) {
    asm!("movl $$0x48+90*4, %eax
          movl $0, %gs:(%eax)" :: "r"(limit) : "eax" : "volatile")
}

#[cfg(all(target_arch = "x86",
          any(target_os = "linux",
              target_os = "freebsd")))]
#[inline(always)]
pub unsafe fn record_sp_limit(limit: usize) {
    asm!("movl $0, %gs:48" :: "r"(limit) :: "volatile")
}

// mips, arm - Some brave soul can port these to inline asm, but it's over
//             my head personally
#[cfg(any(target_arch = "mips",
          target_arch = "mipsel",
          all(target_arch = "arm",
              not(target_os = "ios"))))]
#[inline(always)]
pub unsafe fn record_sp_limit(limit: usize) {
    use libc::c_void;
    return record_sp_limit(limit as usize);
    extern {
        fn record_sp_limit(limit: usize);
    }
}

// FIXME(AARCH64, POWERPC, IOS, OPENBSD, BITRIG): missing...
#[cfg(any(target_arch = "aarch64",
          target_arch = "powerpc",
          all(target_arch = "arm",
              target_os = "ios"),
          target_os = "bitrig",
          target_os = "openbsd"))]
#[inline(always)]
pub unsafe fn record_sp_limit(_: usize) {
}


// glibc >= 2.15 has a __pthread_get_minstack() function that returns
// PTHREAD_STACK_MIN plus however many bytes are needed for thread-local
// storage.  We need that information to avoid blowing up when a small stack
// is created in an application with big thread-local storage requirements.
// See #6233 for rationale and details.
//
// Link weakly to the symbol for compatibility with older versions of glibc.
// Assumes that we've been dynamically linked to libpthread but that is
// currently always the case.  Note that you need to check that the symbol
// is non-null before calling it!
#[cfg(target_os = "linux")]
#[inline(always)]
pub fn min_stack_size(attr: *const libc::pthread_attr_t) -> libc::size_t {
    type F = unsafe extern "C" fn(*const libc::pthread_attr_t) -> libc::size_t;
    extern {
        #[linkage = "extern_weak"]
        static __pthread_get_minstack: *const ();
    }
    if __pthread_get_minstack.is_null() {
        libc::consts::os::posix01::PTHREAD_STACK_MIN
    } else {
        unsafe { mem::transmute::<*const (), F>(__pthread_get_minstack)(attr) }
    }
}

// __pthread_get_minstack() is marked as weak but extern_weak linkage is
// not supported on OS X, hence this kludge...
#[cfg(not(target_os = "linux"))]
#[inline(always)]
pub fn min_stack_size(_: *const libc::pthread_attr_t) -> libc::size_t {
    libc::consts::os::posix01::PTHREAD_STACK_MIN
}

pub type sighandler_t = *const libc::c_void;

#[cfg(any(all(target_os = "linux", target_arch = "x86"), // may not match
          all(target_os = "linux", target_arch = "x86_64"),
          all(target_os = "linux", target_arch = "arm"), // may not match
          all(target_os = "linux", target_arch = "aarch64"),
          all(target_os = "linux", target_arch = "mips"), // may not match
          all(target_os = "linux", target_arch = "mipsel"), // may not match
          all(target_os = "linux", target_arch = "powerpc"), // may not match
          target_os = "android"))] // may not match
mod signal {
    use libc;
    pub use super::sighandler_t;

    pub static SA_ONSTACK: libc::c_int = 0x08000000;
    pub static SA_SIGINFO: libc::c_int = 0x00000004;
    pub static SIGBUS: libc::c_int = 7;

    pub static SIGSTKSZ: libc::size_t = 8192;

    pub const SIG_DFL: sighandler_t = 0 as sighandler_t;

    // This definition is not as accurate as it could be, {si_addr} is
    // actually a giant union. Currently we're only interested in that field,
    // however.
    #[repr(C)]
    pub struct siginfo {
        si_signo: libc::c_int,
        si_errno: libc::c_int,
        si_code: libc::c_int,
        pub si_addr: *mut libc::c_void
    }

    #[repr(C)]
    pub struct sigaction {
        pub sa_sigaction: sighandler_t,
        pub sa_mask: sigset_t,
        pub sa_flags: libc::c_int,
        sa_restorer: *mut libc::c_void,
    }

    #[cfg(target_pointer_width = "32")]
    #[repr(C)]
    pub struct sigset_t {
        __val: [libc::c_ulong; 32],
    }
    #[cfg(target_pointer_width = "64")]
    #[repr(C)]
    pub struct sigset_t {
        __val: [libc::c_ulong; 16],
    }

    #[repr(C)]
    pub struct sigaltstack {
        pub ss_sp: *mut libc::c_void,
        pub ss_flags: libc::c_int,
        pub ss_size: libc::size_t
    }

}

#[cfg(any(target_os = "macos",
          target_os = "bitrig",
          target_os = "openbsd",
          target_os = "freebsd"))]
mod signal {
    use libc;
    pub use super::sighandler_t;

    pub const SA_ONSTACK: libc::c_int = 0x0001;
    pub const SA_SIGINFO: libc::c_int = 0x0040;
    pub const SIGBUS: libc::c_int = 10;

    #[cfg(target_os = "macos")]
    pub const SIGSTKSZ: libc::size_t = 131072;
    #[cfg(any(target_os = "bitrig", target_os = "openbsd", target_os="freebsd"))]
    pub const SIGSTKSZ: libc::size_t = 40960;

    pub const SIG_DFL: sighandler_t = 0 as sighandler_t;

    pub type sigset_t = u32;

    // This structure has more fields, but we're not all that interested in them.
    #[cfg(any(target_os = "macos", target_os = "freebsd"))]
    #[repr(C)]
    pub struct siginfo {
        pub si_signo: libc::c_int,
        pub si_errno: libc::c_int,
        pub si_code: libc::c_int,
        pub pid: libc::pid_t,
        pub uid: libc::uid_t,
        pub status: libc::c_int,
        pub si_addr: *mut libc::c_void
    }

    #[cfg(any(target_os = "bitrig", target_os = "openbsd"))]
    #[repr(C)]
    pub struct siginfo {
        pub si_signo: libc::c_int,
        pub si_code: libc::c_int,
        pub si_errno: libc::c_int,
        //union
        pub si_addr: *mut libc::c_void
    }

    #[repr(C)]
    pub struct sigaltstack {
        pub ss_sp: *mut libc::c_void,
        pub ss_size: libc::size_t,
        pub ss_flags: libc::c_int
    }

    #[cfg(not(target_os = "freebsd"))]
    #[repr(C)]
    pub struct sigaction {
        pub sa_sigaction: sighandler_t,
        pub sa_mask: sigset_t,
        pub sa_flags: libc::c_int,
    }

    #[cfg(target_os = "freebsd")]
    #[repr(C)]
    pub struct sigaction {
        pub sa_sigaction: sighandler_t,
        pub sa_flags: libc::c_int,
        pub sa_mask: sigset_t,
    }
}

extern {
    fn pthread_self() -> libc::pthread_t;
    pub fn signal(signum: libc::c_int, handler: sighandler_t) -> sighandler_t;
    pub fn raise(signum: libc::c_int) -> libc::c_int;
    pub fn sigaction(signum: libc::c_int,
                     act: *const signal::sigaction,
                     oldact: *mut signal::sigaction) -> libc::c_int;
    pub fn sigaltstack(ss: *const signal::sigaltstack,
                       oss: *mut signal::sigaltstack) -> libc::c_int;
}

#[cfg(any(target_os = "linux",
          target_os = "android",
          target_os = "freebsd"))]
extern {
    fn pthread_attr_getstack(attr: *const libc::pthread_attr_t,
                             stackaddr: *mut *mut libc::c_void,
                             stacksize: *mut libc::size_t) -> libc::c_int;
    fn pthread_attr_destroy(attr: *mut libc::pthread_attr_t) -> libc::c_int;
}

#[cfg(any(target_os = "linux",
          target_os = "android"))]
extern {
    fn pthread_getattr_np(native: libc::pthread_t,
                          attr: *mut libc::pthread_attr_t) -> libc::c_int;
}

#[cfg(any(target_os = "freebsd"))]
extern {
    fn pthread_attr_init(attr: *mut libc::pthread_attr_t) -> libc::c_int;
    fn pthread_attr_get_np(pid: libc::pthread_t,
                           attr: *mut libc::pthread_attr_t) -> libc::c_int;
}

#[cfg(target_os = "macos")]
extern {
    fn pthread_get_stackaddr_np(thread: libc::pthread_t) -> *mut libc::c_void;
    fn pthread_get_stacksize_np(thread: libc::pthread_t) -> libc::size_t;
}

#[cfg(any(target_os = "openbsd",
          target_os = "bitrig"))]
#[repr(C)]
struct stack_t {
    ss_sp: *mut libc::c_void,
    ss_size: libc::size_t,
    ss_flags: libc::c_int,
}

#[cfg(any(target_os = "openbsd",
          target_os = "bitrig"))]
extern {
    fn pthread_main_np() -> libc::c_uint;
    fn pthread_stackseg_np(thread: pthread_t, sinfo: *mut stack_t) -> libc::c_uint;
}
