#![allow(clippy::empty_line_after_doc_comments)]
/// Stub module for macros.
///
/// In a real project, the proc macros would be defined in a separate
/// proc-macro crate. This is just a placeholder for compilation.

#[cfg(feature = "proc_macros")]
extern crate proc_macro;

#[cfg(feature = "proc_macros")]
/// Placeholder for the module macro
pub fn module(_: proc_macro::TokenStream, _: proc_macro::TokenStream) -> proc_macro::TokenStream {
    unimplemented!(
        "This is a stub. The real module macro should be implemented in a separate proc-macro crate."
    )
}
