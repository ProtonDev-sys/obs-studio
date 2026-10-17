//! The isolation runner must forward failures and reject an empty test filter.

use super::common;

#[test]
fn child_panic_is_reported_to_the_parent() {
    const TEST: &str = "process_isolation::child_panic_is_reported_to_the_parent";
    if common::is_child(TEST) {
        panic!("isolated child failure probe");
    }

    let panic = std::panic::catch_unwind(|| common::in_child(TEST, || unreachable!()))
        .expect_err("the isolated child's panic must fail its parent");
    let message = panic.downcast_ref::<String>().expect("failure diagnostic");
    assert!(message.contains("isolated test"));
    assert!(message.contains("failed"));
    assert!(message.contains("isolated child failure probe"));
}

#[test]
fn an_exact_filter_that_matches_no_test_is_a_failure() {
    let panic = std::panic::catch_unwind(|| {
        common::in_child("process_isolation::no_such_test", || unreachable!());
    })
    .expect_err("libtest's zero-test success must not hide a misspelled name");
    let message = panic.downcast_ref::<String>().expect("failure diagnostic");
    assert!(message.contains("did not complete its body"));
    assert!(message.contains("running 0 tests"));
}
