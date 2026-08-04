//! Owner-authenticated local-management transport primitives.

#[allow(dead_code)]
mod codec;

#[allow(dead_code)]
mod transaction;

#[cfg(unix)]
mod unix;

#[cfg(unix)]
pub(crate) use unix::LocalManagement;
