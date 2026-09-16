//! Low-level data driver primitives.
//!
//! These modules own pools, transactions, and lease helpers.
//! Domain repository logic belongs in `repository/*`, and application-facing
//! composition belongs in `backend`.

pub mod postgres;
