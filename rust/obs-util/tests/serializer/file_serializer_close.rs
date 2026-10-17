//! A safe serializer must preserve the destination when its final close fails.
//!
//! The C reference checks `fclose`, independently of `ferror`. Interpose the
//! Rust file's `close` to report a delayed error after really closing it. The
//! injection is thread-local and only matches this test's temporary files,
//! so the other tests in the serializer binary can continue in parallel.
#![cfg(target_os = "linux")]

use std::cell::Cell;
use std::ffi::{CString, c_int};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use obs_c_oracle as _;
use obs_util::ffi::array_serializer::serializer;
use obs_util::ffi::file_serializer::{
    file_output_serializer_free, file_output_serializer_init_safe,
};
use obs_util::file_serializer::Output;

const MARKER: &str = "obs-serializer-close-error";
const ORIGINAL: &[u8] = b"original file";
const REPLACEMENT: &[u8] = b"replacement file";

#[derive(Clone, Copy)]
struct CloseState {
    error: c_int,
    attempts: usize,
    matched_fd: Option<c_int>,
}

thread_local! {
    static CLOSE_STATE: Cell<Option<CloseState>> = const { Cell::new(None) };
}

/// Close the real descriptor once, then report the requested error on the
/// first close of the marked temporary file by the thread running the test.
#[unsafe(no_mangle)]
pub extern "C" fn close(fd: c_int) -> c_int {
    let error = CLOSE_STATE
        .try_with(|slot| {
            let mut state = slot.get()?;
            match state.matched_fd {
                Some(matched_fd) if fd != matched_fd => return None,
                Some(_) => (),
                None => {
                    let path = std::fs::read_link(format!("/proc/self/fd/{fd}")).ok()?;
                    if !path.to_string_lossy().contains(MARKER)
                        || path.extension()?.as_bytes() != b"tmp"
                    {
                        return None;
                    }
                    // Keep matching retries after the real close removes the
                    // descriptor's entry from /proc/self/fd.
                    state.matched_fd = Some(fd);
                }
            }
            state.attempts += 1;
            slot.set(Some(state));
            (state.attempts == 1 && state.error != 0).then_some(state.error)
        })
        .ok()
        .flatten();

    // SAFETY: forward the descriptor unchanged to the real close syscall;
    // this bypasses this interposed symbol and never retries a failed close.
    let result = unsafe { libc::syscall(libc::SYS_close, fd) } as c_int;
    if let Some(error) = error {
        // SAFETY: errno is the current thread's writable libc error slot.
        unsafe { *libc::__errno_location() = error };
        -1
    } else {
        result
    }
}

struct Injection;

impl Injection {
    fn new(error: c_int) -> Self {
        CLOSE_STATE.with(|slot| {
            assert!(slot.get().is_none());
            slot.set(Some(CloseState {
                error,
                attempts: 0,
                matched_fd: None,
            }));
        });
        Self
    }

    fn attempts(&self) -> usize {
        CLOSE_STATE.with(|slot| slot.get().unwrap().attempts)
    }
}

impl Drop for Injection {
    fn drop(&mut self) {
        CLOSE_STATE.with(|slot| slot.set(None));
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "{MARKER}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn check_safe_close(error: c_int, existing: bool, through_ffi: bool) {
    let scratch = Scratch::new();
    let destination = scratch.0.join("output.bin");
    let temporary = scratch.0.join("output.bin.tmp");
    if existing {
        std::fs::write(&destination, ORIGINAL).unwrap();
    }

    let attempts = if through_ffi {
        let path = CString::new(destination.as_os_str().as_bytes()).unwrap();
        let mut output = serializer {
            data: std::ptr::null_mut(),
            read: None,
            write: None,
            seek: None,
            get_pos: None,
        };
        // SAFETY: output is writable, both strings are terminated, and a
        // successful init owns the serializer until the single free below.
        assert!(unsafe {
            file_output_serializer_init_safe(&mut output, path.as_ptr(), c"tmp".as_ptr())
        });
        // SAFETY: init installed the write callback; REPLACEMENT covers its length.
        assert_eq!(
            unsafe {
                output.write.unwrap()(output.data, REPLACEMENT.as_ptr().cast(), REPLACEMENT.len())
            },
            REPLACEMENT.len()
        );
        let injection = Injection::new(error);
        // SAFETY: output was initialized successfully and is freed exactly once.
        unsafe { file_output_serializer_free(&mut output) };
        injection.attempts()
    } else {
        let mut output = Output::create_safe(destination.as_os_str().as_bytes(), b"tmp").unwrap();
        assert_eq!(output.write(REPLACEMENT), REPLACEMENT.len());
        let injection = Injection::new(error);
        drop(output);
        injection.attempts()
    };

    assert_eq!(
        attempts, 1,
        "close must be attempted once, without retrying"
    );
    if error == 0 {
        assert_eq!(std::fs::read(&destination).unwrap(), REPLACEMENT);
        assert!(!temporary.exists());
    } else {
        if existing {
            assert_eq!(std::fs::read(&destination).unwrap(), ORIGINAL);
        } else {
            assert!(!destination.exists(), "a failed file must not be promoted");
        }
        assert_eq!(std::fs::read(&temporary).unwrap(), REPLACEMENT);
    }
}

#[test]
fn safe_close_error_keeps_existing_destination() {
    check_safe_close(libc::EIO, true, false);
}

#[test]
fn safe_close_error_does_not_create_destination() {
    check_safe_close(libc::EIO, false, false);
}

#[test]
fn interrupted_close_is_not_retried_or_committed() {
    check_safe_close(libc::EINTR, true, false);
}

#[test]
fn ffi_safe_close_error_keeps_existing_destination() {
    check_safe_close(libc::EIO, true, true);
}

#[test]
fn successful_close_promotes_the_temporary_file() {
    check_safe_close(0, true, false);
    check_safe_close(0, true, true);
}
