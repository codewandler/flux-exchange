//! The connector catalogue, served.
//!
//! `GET /api/catalogue/connectors` lists what the binary was compiled with, and
//! `GET /api/catalogue/connectors/{id}/operations` returns one connector's operations with the
//! `risk`, `effects` and `idempotency` a [`Selector`](exchange_host::Selector) is written over.
//!
//! # Two things this answers, and one it does not
//!
//! It answers **what exists** and **what each operation declares**. It does *not* answer what the
//! caller may run: every operation carries `admitted: null`, and nothing is ever filtered out for
//! want of a grant. [`view::OperationView::admitted`] has the argument.
//!
//! [`view`] holds the whole response contract as pure data, so the shape is tested without a
//! transport. The routes are a thin projection of it.

// TEMPORARY, and it comes out with the route table (X-06 piece 2).
//
// This is a binary crate, so `pub` does not exempt an item from `dead_code`: reachability is
// measured from `main`, and nothing reaches here until the module is added to `routes::MODULES`.
// The alternative was to wire it now, in the one file X-02 is concurrently rewriting, and a merge
// conflict in the route assembly is a worse trade than an attribute with an expiry condition
// written on it. The tests below `view` exercise every item this silences.
#![allow(dead_code)]

pub mod view;
