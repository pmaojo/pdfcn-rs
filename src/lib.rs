//! Plumbing shared by `/api/generate-pdf` and `/api/generate-pdf-batch`.
//!
//! Both handlers need the same optional shared-secret gate and the same
//! SSRF-guarded remote-image fetcher; before this crate existed that code
//! was kept byte-for-byte identical by hand across the two files (the `api`
//! package is bin-only, so there was no lib target to share it through).
//! This is that lib target.

pub mod auth;
pub mod dto;
pub mod remote_image;
