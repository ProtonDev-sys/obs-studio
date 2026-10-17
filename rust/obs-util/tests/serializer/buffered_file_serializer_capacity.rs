//! Buffered writes must make progress even when a requested chunk would fill
//! the queue by itself, leaving no room for its header. These tests exercise
//! the writer and the finished file, with a subprocess deadline for regressions
//! that would otherwise leave a producer and consumer waiting on each other.

use std::process::Command;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use obs_c_oracle as _;
use obs_util::buffered_file_serializer::{Output, SeekType};

#[path = "../common/mod.rs"]
mod common;

pub(super) const MIB: usize = 1_048_576;
const CHILD: &str = "OBS_BUFFERED_CAPACITY_TEST_CHILD";

struct Deadline {
    done: Option<Sender<()>>,
    watchdog: Option<JoinHandle<()>>,
}

impl Deadline {
    fn new() -> Self {
        let (done, completed) = mpsc::channel();
        let watchdog = thread::spawn(move || {
            if completed.recv_timeout(Duration::from_secs(15)) == Err(RecvTimeoutError::Timeout) {
                eprintln!("buffered serializer did not complete within 15 seconds");
                // This guard exists only in the isolated child. Ending the
                // process also ends the blocked producer and I/O threads.
                std::process::exit(124);
            }
        });
        Self {
            done: Some(done),
            watchdog: Some(watchdog),
        }
    }
}

impl Drop for Deadline {
    fn drop(&mut self) {
        // Disconnect also wakes the watchdog when the test unwinds.
        drop(self.done.take());
        if let Some(watchdog) = self.watchdog.take() {
            watchdog.join().expect("deadline watchdog panicked");
        }
    }
}

pub(super) fn isolated(test: &str, run: impl FnOnce()) {
    let completion = format!("completed buffered serializer regression: {test}");
    if std::env::var(CHILD).ok().as_deref() == Some(test) {
        let _deadline = Deadline::new();
        run();
        println!("{completion}");
        return;
    }

    // The parent owns all child scratch files, including on timeout, when the
    // child cannot run its own destructors. Environment changes are child-only.
    let scratch = common::ScratchDir::new("obs-buffered-capacity-child");
    let output = Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", test, "--nocapture"])
        .env(CHILD, test)
        .env("TMPDIR", scratch.path())
        .env("TMP", scratch.path())
        .env("TEMP", scratch.path())
        .output()
        .expect("start buffered serializer regression child");
    assert!(
        output.status.success(),
        "{test}: child status {} (124 means timeout)\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(&completion),
        "{test}: child did not execute the requested regression",
    );
}

fn roundtrip(buffer: usize, chunk: usize, length: usize) {
    let scratch = common::ScratchDir::new("obs-buffered-capacity");
    let path = scratch.join("output.bin");
    let mut expected: Vec<u8> = (0..length).map(|i| (i ^ (i >> 8)) as u8).collect();
    let output = Output::create(&path, buffer, chunk).expect("create buffered serializer");
    assert_eq!(output.pos(), 0);
    assert_eq!(output.write(&expected), expected.len());
    assert_eq!(output.pos(), length as i64);

    let middle = length / 2;
    assert_eq!(
        output.seek(middle as i64, Some(SeekType::Start)),
        middle as i64,
    );
    assert_eq!(output.write(b"new"), 3);
    assert_eq!(output.pos(), (middle + 3) as i64);
    expected[middle..middle + 3].copy_from_slice(b"new");
    drop(output);
    assert_eq!(std::fs::read(path).unwrap(), expected);
}

/// Mirrors `chunk_equal_to_buffer` in test_buffered_serializer_capacity.c.
#[test]
fn chunk_equal_to_buffer() {
    isolated(
        "buffered_file_serializer_capacity::chunk_equal_to_buffer",
        || {
            roundtrip(MIB, MIB, 3 * MIB + 37);
        },
    );
}

/// Mirrors `default_chunk_with_small_buffer` in the C test.
#[test]
fn default_chunk_with_small_buffer() {
    isolated(
        "buffered_file_serializer_capacity::default_chunk_with_small_buffer",
        || roundtrip(MIB, 0, MIB + 37),
    );
}

/// Mirrors `chunk_larger_than_buffer` in the C test.
#[test]
fn chunk_larger_than_buffer() {
    isolated(
        "buffered_file_serializer_capacity::chunk_larger_than_buffer",
        || {
            roundtrip(MIB, 4 * MIB, 4 * MIB + 37);
        },
    );
}

/// Mirrors `small_chunks_keep_working` in the C test.
#[test]
fn small_chunks_keep_working() {
    isolated(
        "buffered_file_serializer_capacity::small_chunks_keep_working",
        || {
            roundtrip(64, 8, 1000);
        },
    );
}

/// Mirrors `defaults_keep_working` in the C test.
#[test]
fn defaults_keep_working() {
    isolated(
        "buffered_file_serializer_capacity::defaults_keep_working",
        || {
            roundtrip(0, 0, 2 * MIB + 37);
        },
    );
}
