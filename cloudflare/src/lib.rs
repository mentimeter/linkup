#![forbid(unsafe_code)]
#![allow(clippy::needless_lifetimes)]

pub mod endpoints;
pub mod framework;

/// Linkup <-> Cloudflare specific features. Changes here will not be upstreamed to cloudflare-rs.
pub mod linkup;
