//! Pass/fail accumulator and per-row reporting.
//!
//! SPDX-License-Identifier: MIT

use std::io::IsTerminal;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy)]
pub enum Outcome {
    Pass,
    Fail,
}

pub struct Runner {
    passed: u32,
    failed: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct Summary {
    pub passed: u32,
    pub failed: u32,
}

impl Summary {
    pub fn total(&self) -> u32 {
        self.passed + self.failed
    }
    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }
}

impl Default for Runner {
    fn default() -> Self {
        Self::new()
    }
}

impl Runner {
    pub fn new() -> Self {
        Self { passed: 0, failed: 0 }
    }

    pub fn record(&mut self, line: usize, name: &str, outcome: Outcome, detail: &str) {
        match outcome {
            Outcome::Pass => {
                self.passed += 1;
                println!("[test] {} L{line}: {name}", paint(GREEN, "PASS"));
            }
            Outcome::Fail => {
                self.failed += 1;
                println!("[test] {} L{line}: {name} -- {detail}", paint(RED, "FAIL"),);
            }
        }
    }

    pub fn into_summary(self) -> Summary {
        Summary {
            passed: self.passed,
            failed: self.failed,
        }
    }
}

// ── ANSI coloring ──────────────────────────────────────────────────────
//
// Auto-enabled when stdout is a TTY; disabled under file redirection,
// piping, and CI logs. Override with `NO_COLOR=1` (per https://no-color.org).

const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";

fn color_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal())
}

fn paint(code: &str, text: &str) -> String {
    if color_enabled() {
        format!("{code}{text}{RESET}")
    } else {
        text.to_string()
    }
}
