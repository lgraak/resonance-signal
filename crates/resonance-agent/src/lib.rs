//! Platform capture orchestration for the Resonance Signal provider.

pub mod capture;
pub mod protocol;
// Discovery operations remain agent-private until a consumer transport is selected.
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
pub mod startup;

#[cfg(windows)]
pub mod transport;

#[cfg(windows)]
pub mod tray;

#[cfg(windows)]
pub mod windows;

#[cfg(windows)]
#[allow(dead_code)]
mod windows_discovery;
