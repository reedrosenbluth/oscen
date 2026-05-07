//! Boundary diagnostic types for the graph compiler.
//!
//! Today these wrap a single `syn::Error` per compile attempt, but the
//! `Diagnostics` shape is plural so future passes can accumulate
//! multiple errors without breaking API consumers.

use proc_macro2::TokenStream;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug)]
pub struct Diagnostic {
    pub error: syn::Error,
    pub severity: Severity,
}

impl Diagnostic {
    pub fn error(error: syn::Error) -> Self {
        Self {
            error,
            severity: Severity::Error,
        }
    }

    pub fn warning(error: syn::Error) -> Self {
        Self {
            error,
            severity: Severity::Warning,
        }
    }
}

#[derive(Debug, Default)]
pub struct Diagnostics {
    pub items: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_error(&mut self, e: syn::Error) {
        self.items.push(Diagnostic::error(e));
    }

    pub fn push_warning(&mut self, e: syn::Error) {
        self.items.push(Diagnostic::warning(e));
    }

    pub fn extend_from_syn(&mut self, errs: impl IntoIterator<Item = syn::Error>) {
        for e in errs {
            self.push_error(e);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Collapse all contained errors into a single `compile_error!` token
    /// stream by combining them via `syn::Error::combine`. Warnings are
    /// dropped for now (Phase 2a does not produce any).
    ///
    /// If `self` is empty or contains only warnings, returns an empty
    /// `TokenStream`. Callers should only return `Err(diags)` from
    /// `compile()` when at least one error is present — otherwise the
    /// proc-macro host sees a silent successful expansion.
    pub fn into_compile_errors(self) -> TokenStream {
        let mut combined: Option<syn::Error> = None;
        for d in self.items {
            if matches!(d.severity, Severity::Error) {
                match combined.as_mut() {
                    Some(acc) => acc.combine(d.error),
                    None => combined = Some(d.error),
                }
            }
        }
        match combined {
            Some(e) => e.to_compile_error(),
            None => TokenStream::new(),
        }
    }
}

impl From<syn::Error> for Diagnostics {
    fn from(e: syn::Error) -> Self {
        let mut d = Self::new();
        d.push_error(e);
        d
    }
}
