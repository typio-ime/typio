//! The unit of vetting output: a categorized check with a pass/warn/fail verdict.

use std::fmt;

/// Which dimension of the engine contract a check belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckCategory {
    /// Structural / ABI surface: `TypioEngineInfo`, struct sizes, vtables.
    Abi,
    /// Runtime behavior observed by driving the engine through the mock host.
    Behavior,
    /// Packaged assets that ship alongside the native engine artifact (icons,
    /// etc.).
    Resource,
}

impl CheckCategory {
    pub fn label(self) -> &'static str {
        match self {
            CheckCategory::Abi => "ABI",
            CheckCategory::Behavior => "Behavior",
            CheckCategory::Resource => "Resource",
        }
    }
}

/// The verdict of a single check.
///
/// `Warn` is the load-bearing addition over a plain pass/fail: it lets `vet`
/// surface behavior that is *suspicious but legal* (e.g. a keyboard engine that
/// declines a plain ASCII key) without failing the gate and without silently
/// rubber-stamping a do-nothing engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

impl CheckStatus {
    pub fn label(self) -> &'static str {
        match self {
            CheckStatus::Pass => "PASS",
            CheckStatus::Warn => "WARN",
            CheckStatus::Fail => "FAIL",
        }
    }
}

impl fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub category: CheckCategory,
    pub name: &'static str,
    pub status: CheckStatus,
    pub detail: String,
}

impl CheckResult {
    pub fn pass(category: CheckCategory, name: &'static str) -> Self {
        CheckResult {
            category,
            name,
            status: CheckStatus::Pass,
            detail: String::new(),
        }
    }

    pub fn warn(category: CheckCategory, name: &'static str, detail: impl Into<String>) -> Self {
        CheckResult {
            category,
            name,
            status: CheckStatus::Warn,
            detail: detail.into(),
        }
    }

    pub fn fail(category: CheckCategory, name: &'static str, detail: impl Into<String>) -> Self {
        CheckResult {
            category,
            name,
            status: CheckStatus::Fail,
            detail: detail.into(),
        }
    }

    pub fn passed(&self) -> bool {
        self.status == CheckStatus::Pass
    }

    pub fn failed(&self) -> bool {
        self.status == CheckStatus::Fail
    }
}

/// Tally of a check run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    pub passed: usize,
    pub warned: usize,
    pub failed: usize,
}

impl Summary {
    pub fn of(results: &[CheckResult]) -> Self {
        let mut s = Summary::default();
        for r in results {
            match r.status {
                CheckStatus::Pass => s.passed += 1,
                CheckStatus::Warn => s.warned += 1,
                CheckStatus::Fail => s.failed += 1,
            }
        }
        s
    }

    /// The vet gate only fails on hard failures; warnings do not block.
    pub fn is_failure(&self) -> bool {
        self.failed > 0
    }
}
