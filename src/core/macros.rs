//! Core Macros
//!
//! This module defines utility macros used throughout the HRV analysis tool.
//! These macros simplify common patterns and reduce boilerplate code.

/// `map_err` macro.
///
/// This macro simplifies `map_err` functionality by converting errors into strings
/// to ensure consistent error handling throughout the application.
#[macro_export]
macro_rules! map_err {
    ($result:expr) => {
        $result.map_err(|e| e.to_string())
    };
}
