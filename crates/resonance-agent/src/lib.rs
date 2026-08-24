//! Platform capture orchestration for the Resonance Signal provider.

pub mod capture;

pub(crate) mod recovery;

#[cfg(windows)]
pub mod supervisor;

#[cfg(windows)]
pub mod windows;
