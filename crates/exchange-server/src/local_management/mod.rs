//! Owner-authenticated local-management transport primitives.

#[allow(dead_code)]
mod codec;

pub mod transaction;

pub use transaction::TransactionCoordinator;

#[allow(dead_code)]
mod service_account_handoff;

#[cfg(unix)]
mod unix;

#[cfg(unix)]
pub(crate) use unix::LocalManagement;
