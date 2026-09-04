//! Typed contracts for the unified terminal dashboard.
//!
//! These contracts are additive while the loopback dashboard remains the
//! active implementation. They deliberately live below a namespace so the
//! cutover can coexist with the legacy root-level web DTOs until routing moves.

mod bounded;
mod identity;
mod parity;
mod recorder;
mod source;
mod status;

#[cfg(any(test, feature = "test-support"))]
pub mod scenarios;

pub use bounded::*;
pub use identity::*;
pub use parity::*;
pub use recorder::*;
pub use source::*;
pub use status::*;
