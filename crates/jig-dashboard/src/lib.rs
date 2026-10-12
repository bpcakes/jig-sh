//! Typed contracts for the unified terminal dashboard.
//!
//! The CLI and loops own repository access and supply data through these
//! contracts; this crate owns their bounded projection. `jig-ui` renders them.

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
