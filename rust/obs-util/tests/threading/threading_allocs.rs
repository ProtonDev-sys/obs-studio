//! `bnum_allocs` accounting for the threading handles: the C allocates
//! `struct os_event_data`/`struct os_sem_data` with `bzalloc`, so every
//! live handle is one allocation unit. In a `Box`-allocated port the
//! counter never moves.
//!
//! `bnum_allocs` is a process-global counter, so asserting absolute deltas
//! must be isolated from other tests in the shared threading binary.
//!
//! Unix only, like the symbols it exercises.

#![cfg(unix)]

use std::ptr;
use std::sync::mpsc;
use std::thread;

use obs_c_oracle as _; // links the oracle test stubs
use obs_util::ffi::bmem::{bfree, bmalloc, bnum_allocs};
use obs_util::ffi::threading::{os_event_destroy, os_event_init, os_sem_destroy, os_sem_init};
use obs_util::threading::{Event, Sem};

#[test]
fn init_destroy_counts_one_bzalloc_per_live_handle() {
    check_handle_allocations(|| ());
}

fn check_handle_allocations<T>(after_base: impl FnOnce() -> T) {
    super::common::in_child(
        "threading_allocs::init_destroy_counts_one_bzalloc_per_live_handle",
        || {
            // SAFETY: the allocation counter is always readable.
            let base = unsafe { bnum_allocs() };
            let _sibling = after_base();

            let mut event: *mut Event = ptr::null_mut();
            // SAFETY: `event` is a writable handle slot; the counter is readable.
            unsafe {
                assert_eq!(os_event_init(&mut event, 0), 0);
                assert_eq!(bnum_allocs(), base + 1);
            }

            let mut sem: *mut Sem = ptr::null_mut();
            // SAFETY: `sem` is a writable handle slot; the counter is readable.
            unsafe {
                assert_eq!(os_sem_init(&mut sem, 0), 0);
                assert_eq!(bnum_allocs(), base + 2);
            }

            // SAFETY: both handles were initialized successfully above.
            unsafe {
                os_event_destroy(event);
                os_sem_destroy(sem);
            }
            // SAFETY: the allocation counter is always readable.
            assert_eq!(unsafe { bnum_allocs() }, base);
        },
    );
}

/// Hold another test's allocation across the actual handle measurements.
/// The rendezvous makes the regression deterministic without sleeps or retries.
#[test]
fn handle_counts_are_isolated_from_sibling_allocations() {
    check_handle_allocations(SiblingAllocation::new);
}

struct SiblingAllocation {
    finish: Option<mpsc::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl SiblingAllocation {
    fn new() -> Self {
        let (ready_tx, ready_rx) = mpsc::sync_channel(0);
        let (finish_tx, finish_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            // SAFETY: the nonzero allocation is owned by this worker and freed below.
            let allocation = unsafe { bmalloc(1) };
            ready_tx.send(()).unwrap();
            let _ = finish_rx.recv();
            // SAFETY: `allocation` is still live and is freed exactly once.
            unsafe { bfree(allocation) };
        });
        ready_rx.recv().unwrap();
        Self {
            finish: Some(finish_tx),
            worker: Some(worker),
        }
    }
}

impl Drop for SiblingAllocation {
    fn drop(&mut self) {
        // Closing the channel releases the worker even if a count assertion panics.
        drop(self.finish.take());
        self.worker.take().unwrap().join().unwrap();
    }
}
