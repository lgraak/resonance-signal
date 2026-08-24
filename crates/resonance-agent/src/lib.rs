//! Platform capture orchestration for the Resonance Signal provider.

pub mod capture;

#[cfg(windows)]
pub mod supervisor;

#[cfg(windows)]
pub mod windows;
