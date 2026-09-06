//! Typed contracts for the unified terminal dashboard.
//!
//! The CLI owns repository access and supplies data through these contracts;
//! this crate owns only bounded projection and presentation.

mod bounded;
mod identity;
#[cfg(any(test, feature = "test-support"))]
mod parity;
mod recorder;
mod source;
mod status;

#[cfg(any(test, feature = "test-support"))]
pub mod scenarios;

pub use bounded::*;
pub use identity::*;
#[cfg(any(test, feature = "test-support"))]
pub use parity::*;
pub use recorder::*;
pub use source::*;
pub use status::*;
