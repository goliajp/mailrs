//! Tests for `reputation` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn health_classification() {
    assert_eq!(classify_health(0.0), "good");
    assert_eq!(classify_health(0.03), "good");
    assert_eq!(classify_health(0.05), "warning");
    assert_eq!(classify_health(0.08), "warning");
    assert_eq!(classify_health(0.10), "critical");
    assert_eq!(classify_health(0.25), "critical");
}
