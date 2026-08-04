//! Owner-authenticated local-management transport primitives.

#[allow(dead_code)]
mod codec;

mod dispatcher;
mod grant;

pub mod transaction;

pub(crate) use dispatcher::Dispatcher;
pub(crate) use dispatcher::Transport;
pub use transaction::TransactionCoordinator;

#[allow(dead_code)]
mod service_account_handoff;

#[cfg(unix)]
mod unix;

#[cfg(unix)]
pub(crate) use unix::LocalManagement;
