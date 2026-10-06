//! Own binary so the shared helper adds no test to count-gated comparators.
#[path = "support/go_source_pin.rs"]
mod go_source_pin;

use go_source_pin::current_tree_bound;

#[test]
fn only_unrecorded_go_tests_leave_the_current_tree_pin() {
    assert!(current_tree_bound("cmd/symeraseme/main.go"));
    assert!(current_tree_bound("Cargo.lock"));
    assert!(!current_tree_bound(".gitattributes"));
    assert!(current_tree_bound(
        "internal/campaign/plan_bytes_oracle_test.go"
    ));
    assert!(current_tree_bound(
        "rust-tests/parity/oracle/storage/main_test.go"
    ));
    assert!(!current_tree_bound(
        "cmd/symeraseme/release_pipeline_test.go"
    ));
    assert!(!current_tree_bound("internal/campaign/nested/plan_test.go"));
}
