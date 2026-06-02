//! Smoke test: every bundled example script *parses* and *executes*
//! against the in-process Mock source without crashing.
//!
//! Note: rows are allowed to fail. The bundled scripts target real
//! hardware (see each file's header) and assert physical invariants —
//! Mock returns synthetic constants and a fixed clock, so e.g. the
//! "clock advanced after sleep" rows and "threshold round-trip readback
//! equals what we just wrote" rows will not all pass. This test only
//! guards against parser regressions, dispatcher panics, and missing
//! method wiring.
//!
//! SPDX-License-Identifier: MIT

use ec_test_lib::mock::Mock;
use ec_test_script::run_script;

fn parse_and_run(name: &str, script: &str) {
    let mock = Mock::default();
    let summary = run_script(&mock, script).unwrap_or_else(|e| panic!("{name}: parse: {e}"));
    assert!(summary.total() > 0, "{name}: no rows executed");
}

#[test]
fn thermal() {
    parse_and_run("thermal", include_str!("../examples/thermal.test"));
}

#[test]
fn battery() {
    parse_and_run("battery", include_str!("../examples/battery.test"));
}

#[test]
fn rtc() {
    parse_and_run("rtc", include_str!("../examples/rtc.test"));
}

#[test]
fn full() {
    parse_and_run("full", include_str!("../examples/full.test"));
}

#[test]
fn advanced() {
    parse_and_run("advanced", include_str!("../examples/advanced.test"));
}
