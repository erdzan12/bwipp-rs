//! Internal helpers shared across symbologies.
//!
//! Modules under `util` implement algorithms that more than one symbology
//! needs (Reed-Solomon variants, GS1 check digits, AI parsing) so each
//! symbology file stays focused on its own encoding rules.

pub mod gs1;
pub mod rs_gf113;
pub mod rs_gf2k;
pub mod rs_gf64;
pub mod rs_gf929;
