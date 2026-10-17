//! obs-util integration tests: memory (one module per former tests/*.rs file, #968).
//! Counter parity runs in a child, so other memory tests can run in parallel.

#[path = "../common/mod.rs"]
mod common;

mod bmem;
mod bmem_parity;
mod process_isolation;
