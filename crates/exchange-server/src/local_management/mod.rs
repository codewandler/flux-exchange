//! Owner-authenticated local-management transport primitives.

#[allow(dead_code)]
mod codec;

mod connection;
mod deadline;

mod dispatcher;
mod grant;
pub(crate) mod proposal;
mod service_account;

pub mod transaction;

pub(crate) use deadline::DeadlineController;
#[cfg(any(test, feature = "native-deadline-test-seam"))]
pub(crate) use deadline::{Expired, ReceiptIdentity, Unresolved};
#[cfg(any(test, feature = "native-deadline-test-seam"))]
pub(crate) use dispatcher::deadline_frame;
pub(crate) use dispatcher::expired_reply;
pub(crate) use dispatcher::ActiveSession;
pub(crate) use dispatcher::Dispatcher;
pub(crate) use dispatcher::Transport;
pub(crate) use dispatcher::{SessionAdvance, SessionBegin};
pub use transaction::TransactionCoordinator;

pub(crate) mod service_account_handoff;

#[cfg(unix)]
mod unix;

#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(crate) use unix::LocalManagement;

#[cfg(all(unix, feature = "native-deadline-test-seam"))]
pub(crate) use unix::run_deadline_process_fixture as run_unix_deadline_process_fixture;

#[cfg(windows)]
pub(crate) use windows::LocalManagement;

#[cfg(all(windows, feature = "native-deadline-test-seam"))]
pub(crate) use windows::run_deadline_process_fixture;
