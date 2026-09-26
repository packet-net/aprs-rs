//! Collects diagnostics while decoding, and applies the parse options.

use alloc::string::ToString;
use alloc::vec::Vec;

use crate::{Code, Diagnostic, HeaderError, ParseOptions, Severity};

#[derive(Clone, Debug)]
pub(crate) struct Context {
    pub(crate) options: ParseOptions,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

impl Context {
    pub(crate) fn new(options: ParseOptions) -> Context {
        Context { options, diagnostics: Vec::new() }
    }

    fn push(&mut self, severity: Severity, code: Code, message: &str, offset: Option<usize>) {
        self.diagnostics.push(Diagnostic { severity, code, message: message.to_string(), offset });
    }

    pub(crate) fn info(&mut self, code: Code, message: &str, offset: Option<usize>) {
        self.push(Severity::Info, code, message, offset);
    }

    pub(crate) fn warn(&mut self, code: Code, message: &str, offset: Option<usize>) {
        self.push(Severity::Warning, code, message, offset);
    }

    pub(crate) fn error(&mut self, code: Code, message: &str, offset: Option<usize>) {
        self.push(Severity::Error, code, message, offset);
    }

    /// A defect the options may tolerate: a warning and `true` if they do, an error and `false` if not.
    pub(crate) fn tolerate(&mut self, code: Code, message: &str, offset: Option<usize>) -> bool {
        if self.options.tolerates(code) {
            self.warn(code, message, offset);
            true
        } else {
            self.error(code, message, offset);
            false
        }
    }

    /// Whether an extra reading (one that is not about a defect) is allowed; warns when it is.
    pub(crate) fn allows(&mut self, code: Code, message: &str, offset: Option<usize>) -> bool {
        if self.options.tolerates(code) {
            self.warn(code, message, offset);
            true
        } else {
            false
        }
    }

    pub(crate) fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }

    pub(crate) fn header_error(&self) -> HeaderError {
        HeaderError { diagnostics: self.diagnostics.clone() }
    }
}
