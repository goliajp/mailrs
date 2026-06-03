// Each integration test crate only uses the subset of helpers it
// needs. The unused-in-one-crate items are not dead code in
// aggregate, so silence the dead-code lint at the helper-module root.
#![allow(dead_code)]

pub mod mock_smtp;
pub mod pg;
