//! Bundled element catalog. The compiler checks this document; widgets are
//! dispatched by name in `project`.

/// `catalog/v1.json`. Passed on every compile from `load` and `engine`.
pub(crate) const V1: &str = include_str!("../catalog/v1.json");
