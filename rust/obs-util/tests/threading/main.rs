//! obs-util integration tests: threading (one module per former tests/*.rs file, #968).

#[path = "../common/mod.rs"]
mod common;

mod task;
mod task_parity;
mod task_stress;
mod threading;
mod threading_allocs;
mod threading_parity;
mod threading_stress;
mod threading_windows;
mod threading_windows_parity;
