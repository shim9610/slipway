//! Pins the compiler-feedback quality of the capability-bundle errors
//! (roadmap Phase 1, LE-H7/LE-M23). The `.stderr` snapshots assert the
//! `#[diagnostic::on_unimplemented]` triage (bundle name, LOAD-BEARING vs
//! RESERVED note, `reserved_policy_defaults!` pointer) and the ABSENCE of
//! the misleading `SlipwayAppWidget<A>` wrapper suggestion suppressed by
//! `#[diagnostic::do_not_recommend]`. If a rustc upgrade or a core edit
//! degrades the error text, this test fails; regenerate deliberately with
//! `TRYBUILD=overwrite cargo test -p slipway-core --test compile_fail`
//! and review the diff as an error-message change.
//!
//! Pinning choice: the probe requires the bundle through a bound declared
//! INSIDE the probe file, so the snapshot contains no slipway-core source
//! line numbers (trybuild normalizes the probe paths to `$DIR`); ordinary
//! core edits do not invalidate the snapshot, only genuine error-output
//! changes do.

#[test]
fn bundle_error_snapshots() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/*.rs");
}
