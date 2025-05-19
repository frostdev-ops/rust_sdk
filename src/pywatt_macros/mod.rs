// Placeholder for pywatt_macros integration
// This will be populated when proc_macros feature is enabled

#[cfg(feature = "proc_macros")]
pub mod proc_macro_impl {
    // There should not be a proc macro implementation here
    // This implementation must be moved to a separate crate with crate-type = "proc-macro"
}

// Re-export the module macro when proc_macros feature is enabled
#[cfg(feature = "proc_macros")]
pub use crate::macros::module;
