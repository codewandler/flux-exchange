//! Owner-authenticated local-management transport primitives.

// Kept compiled ahead of the endpoint slice so the provider-owned wire contract cannot drift while
// native transport composition is integrated separately.
#[allow(dead_code)]
mod codec;
