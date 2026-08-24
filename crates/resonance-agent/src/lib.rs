//! Platform capture orchestration for the Resonance Signal provider.

pub mod capture;

pub(crate) mod recovery;
pub(crate) mod recovery_config;
pub(crate) mod retry_state;

#[cfg(windows)]
pub mod supervisor;

#[cfg(windows)]
pub mod windows;
