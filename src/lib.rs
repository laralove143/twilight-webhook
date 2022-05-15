//! Caching and utility methods for Discord webhooks, a third party crate of the
//! [Twilight ecosystem](https://api.twilight.rs)
//!
//! Refer to the modules' docs for more

#![warn(clippy::cargo, clippy::nursery, clippy::pedantic, clippy::restriction)]
#![allow(
    clippy::blanket_clippy_restriction_lints,
    clippy::implicit_return,
    clippy::exhaustive_enums,
    clippy::missing_inline_in_public_items,
    clippy::single_char_lifetime_names,
    clippy::pattern_type_mismatch
)]

/// The webhooks cache
pub mod cache;
/// Various utility functions for webhooks
pub mod util;
