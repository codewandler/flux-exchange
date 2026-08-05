//! Provider-owned local release and supervision wire contracts.
//!
//! The server binary composes these contracts. Flux C-510 consumes Exchange's committed contract
//! and conformance fixtures; this unpublished package does not create a cross-repository dependency.

pub mod entropy;
pub mod protocol_identity;
pub mod service_account;
pub mod supervisor;

#[cfg(windows)]
pub mod windows_handle;
