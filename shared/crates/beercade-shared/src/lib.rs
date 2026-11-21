//! # B.E.E.R.C.A.D.E. Shared Core Library
//!
//! Centralized protocol contracts, authentication primitives, data models,
//! and cryptographic verification utilities shared between B.O.S.S. and A.S.T.R.O.

pub mod auth;
pub mod crypto;
pub mod ipc;
pub mod models;
pub mod proto_ts;
pub mod shell;
pub mod theme;

pub use auth::{constant_time_eq, extract_bearer_token, format_bearer_header, validate_token, NodeClaim};
pub use crypto::{
    compute_reader_sha256, compute_sha256, verify_sha256, verify_sha256_hex, Sha256Hasher,
};
pub use models::*;
