// Copyright 2014 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#![allow(deprecated,unused_imports)]

use core::prelude::*;

use boxed::Box;
use mem;
use usize;
use libc;
use thunk::Thunk;
use sys_common::stack;
use sys::{thread, stack_overflow};

/// Approximate the bounds of the current thread stack. For the main thread stack call it as early
/// after the entry point (preferred _start, main, #[lang="start"] fn in this order) as possible.
#[inline(always)]
pub unsafe fn guess_thread_stack_bounds(is_main: bool) -> (*const (), *const ()) {
    use libc::consts::os::posix01::{PTHREAD_CREATE_JOINABLE, PTHREAD_STACK_MIN};
    use libc;
    use ptr;

    extern {
        #[linkage="extern_weak"]
        static __libc_stack_end: *const *const u8;

        fn pthread_getattr_np(native: libc::pthread_t,
                              attr: *mut libc::pthread_attr_t) -> libc::c_int;
        fn pthread_self() -> libc::pthread_t;
        fn pthread_attr_getstack(attr: *const libc::pthread_attr_t,
                                 stackaddr: *mut *mut libc::c_void,
                                 stacksize: *mut libc::size_t) -> libc::c_int;
        fn pthread_attr_destroy(attr: *mut libc::pthread_attr_t) -> libc::c_int;
    }

    #[cfg(not(stage0))]
    extern "rust-intrinsic" {
        fn frame_address(n: u32) -> *const u8;
    }

    #[cfg(stage0)]
    #[inline(always)]
    fn frame_address(n: u32) -> *const u8 {
        let i: u8 = 0;
        return &i as *const _;
    }

    if !is_main {
        // If the thread is not main, it was created by pthread, and we can just ask pthread to do
        // the grunt work for us here.
        let mut attr: libc::pthread_attr_t = mem::zeroed();
        if pthread_getattr_np(pthread_self(), &mut attr) != 0 {
            panic!("failed to get thread attributes");
        }
        let mut stackaddr = ptr::null_mut();
        let mut stacksize = 0;
        if pthread_attr_getstack(&attr, &mut stackaddr, &mut stacksize) != 0 {
            panic!("failed to get stack information");
        }
        if pthread_attr_destroy(&mut attr) != 0 {
            panic!("failed to destroy thread attributes");
        }
        let stackaddr = stackaddr as *const u8;
        (stackaddr as *const _, stackaddr.offset(stacksize as isize) as *const _)
    } else {
        // We do not want to call out to pthread in order to find out information about the stack
        // of main thread – it reads the filesystem (/proc/self/maps) and might fail in various
        // ways. What we do instead is try many ways to get some end of the stack and calculate
        // another end with the stack size.
        let mut max_stack: libc::rlimit = mem::zeroed();
        if libc::getrlimit(libc::RLIMIT_STACK, &mut max_stack as *mut _) != 0 {
            panic!("failed to get max stack size");
        }
        let max_stack = max_stack.rlim_cur;
        let page_size = ::os::page_size();

        // Not really the address of the end per se. On most systems we have environment variables,
        // and argv strings between actual top of the stack and this address.
        let stack_end = if !__libc_stack_end.is_null() {
            *__libc_stack_end
        } else {
            frame_address(0)
        };

        // Main stack may not be misaligned. This allows us to adjust for the stack used until this
        // function was called.
        let misalignment = (stack_end as usize % page_size) as isize;
        let stack_end = if misalignment != 0 { stack_end.offset(-misalignment) } else { stack_end };

        // These are correct within the size of environment variables and argv strings that are put
        // onto the stack.
        (stack_end.offset(-(max_stack as isize)) as *const _, stack_end as *const _)
    }
}

// This is the starting point of rust os threads. The first thing we do
// is make sure that we don't trigger __morestack (also why this has a
// no_stack_check annotation), and then we extract the main function
// and invoke it.
#[no_stack_check]
pub fn start_thread(main: *mut libc::c_void) -> thread::rust_thread_return {
    unsafe {
        stack::record_os_managed_stack_bounds(0, usize::MAX);
        let handler = stack_overflow::Handler::new();
        let f: Box<Thunk> = Box::from_raw(main as *mut Thunk);
        f.invoke(());
        drop(handler);
        mem::transmute(0 as thread::rust_thread_return)
    }
}
