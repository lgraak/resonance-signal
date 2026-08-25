//! Platform capture orchestration for the Resonance Signal provider.

pub mod capture;
// The discovery boundary remains private until the consumer descriptor API is approved.
#[allow(dead_code)]
mod discovery;
#[allow(dead_code)]
mod identity;

pub(crate) mod recovery;
pub(crate) mod recovery_config;
pub(crate) mod retry_state;

#[cfg(windows)]
pub mod supervisor;

#[cfg(windows)]
pub mod windows;

#[cfg(windows)]
#[allow(dead_code)]
mod windows_discovery;
