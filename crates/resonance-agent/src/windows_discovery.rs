use wasapi::{deinitialize, initialize_mta, DeviceEnumerator, DeviceState, Direction, Role};

use crate::discovery::{
    PlaybackEndpointObservation, PlaybackEndpointSource, PlaybackEndpointState,
};
use crate::identity::ObservedContinuity;

const WINDOWS_ENDPOINT_EVIDENCE_TOKEN: &str = "windows-immdevice-endpoint-id-v1";

pub(crate) struct WindowsPlaybackEndpointSource;

impl WindowsPlaybackEndpointSource {
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl PlaybackEndpointSource for WindowsPlaybackEndpointSource {
    fn enumerate_playback_endpoints(&mut self) -> Result<Vec<PlaybackEndpointObservation>, String> {
        initialize_mta()
            .ok()
            .map_err(|error| format!("failed to initialize COM MTA for discovery: {error}"))?;
        let _com = ComGuard;

        let enumerator = DeviceEnumerator::new()
            .map_err(|error| format!("failed to create WASAPI device enumerator: {error}"))?;
        let collection = enumerator
            .get_device_collection(&Direction::Render)
            .map_err(|error| {
                format!("failed to enumerate active WASAPI playback endpoints: {error}")
            })?;
        let endpoint_count = collection.get_nbr_devices().map_err(|error| {
            format!("failed to count active WASAPI playback endpoints: {error}")
        })?;

        let default_endpoint_id = if endpoint_count == 0 {
            None
        } else {
            let default = enumerator
                .get_default_device_for_role(&Direction::Render, &Role::Console)
                .map_err(|error| {
                    format!("failed to resolve the console default playback endpoint: {error}")
                })?;
            Some(default.get_id().map_err(|error| {
                format!("failed to read the default playback endpoint identity: {error}")
            })?)
        };

        let mut observations = Vec::with_capacity(endpoint_count as usize);
        for index in 0..endpoint_count {
            let endpoint = collection.get_device_at_index(index).map_err(|error| {
                format!("failed to read an active WASAPI playback endpoint: {error}")
            })?;
            let state = endpoint.get_state().map_err(|error| {
                format!("failed to verify a WASAPI playback endpoint state: {error}")
            })?;
            let endpoint_id = endpoint.get_id().map_err(|error| {
                format!("failed to read a WASAPI playback endpoint identity: {error}")
            })?;
            let display_name = endpoint.get_friendlyname().map_err(|error| {
                format!("failed to read a WASAPI playback endpoint name: {error}")
            })?;

            observations.push(PlaybackEndpointObservation {
                display_name,
                state: map_device_state(state),
                continuity: ObservedContinuity::Stable {
                    backend_key: endpoint_id.clone(),
                    continuity_token: WINDOWS_ENDPOINT_EVIDENCE_TOKEN.to_string(),
                },
                is_default_playback: default_endpoint_id
                    .as_ref()
                    .is_some_and(|default_id| default_id == &endpoint_id),
            });
        }

        Ok(observations)
    }
}

fn map_device_state(state: DeviceState) -> PlaybackEndpointState {
    match state {
        DeviceState::Active => PlaybackEndpointState::Active,
        DeviceState::Disabled => PlaybackEndpointState::Disabled,
        DeviceState::NotPresent => PlaybackEndpointState::NotPresent,
        DeviceState::Unplugged => PlaybackEndpointState::Unplugged,
    }
}

struct ComGuard;

impl Drop for ComGuard {
    fn drop(&mut self) {
        deinitialize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::PlaybackDiscovery;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "resonance-windows-discovery-real-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    #[ignore = "requires the host's real Windows playback endpoint set"]
    fn real_windows_playback_discovery_refresh_and_reopen() {
        let directory = TestDirectory::new();
        let mut discovery =
            PlaybackDiscovery::new(&directory.0, WindowsPlaybackEndpointSource::new()).unwrap();
        let first = discovery.refresh().unwrap();
        let portable_first = first.to_portable().unwrap();

        println!(
            "Portable Windows playback discovery: descriptors={}",
            portable_first.sources().len()
        );
        for source in portable_first.sources() {
            println!(
                "  source_id={} | name={} | availability={:?} | default_playback={} | products={:?}",
                source.source_id().as_str(),
                source.display_name().unwrap_or("<unnamed>"),
                source.availability(),
                source.is_default_playback(),
                source.supported_products()
            );
        }
        if !first.sources().is_empty() {
            let resolved = discovery.resolve_default_playback(&first).unwrap();
            println!(
                "  resolved Default Playback source_id={}",
                resolved.as_str()
            );
        }

        let refreshed = discovery.refresh().unwrap();
        assert_eq!(
            first, refreshed,
            "immediate refresh changed endpoint identity"
        );
        drop(discovery);

        let mut reopened =
            PlaybackDiscovery::new(&directory.0, WindowsPlaybackEndpointSource::new()).unwrap();
        let after_reopen = reopened.refresh().unwrap();
        assert_eq!(first.namespace(), after_reopen.namespace());
        assert_eq!(first.sources(), after_reopen.sources());
        println!(
            "  refresh stable; registry reopen retained {} SourceId mappings",
            after_reopen.sources().len()
        );
    }
}
