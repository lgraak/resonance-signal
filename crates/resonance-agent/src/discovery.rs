use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{self, Display};
use std::path::Path;

use resonance_api::contract::{
    DefaultSource, DiscoveryContractError, DiscoveryRevision,
    DiscoverySnapshot as PortableDiscoverySnapshot, SignalProduct, SourceAvailability,
    SourceDescriptor as PortableSourceDescriptor, SourceId, SourceKind,
};

use crate::identity::{
    ContinuityEvidence, DiscoverySnapshot as IdentitySnapshot, IdentityRegistry,
    ObservedContinuity, RegistryError, SourceObservation, SourceResolution,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlaybackEndpointState {
    Active,
    Disabled,
    NotPresent,
    Unplugged,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PlaybackEndpointObservation {
    pub(crate) display_name: String,
    pub(crate) state: PlaybackEndpointState,
    pub(crate) continuity: ObservedContinuity,
    pub(crate) is_default_playback: bool,
}

pub(crate) trait PlaybackEndpointSource {
    fn enumerate_playback_endpoints(&mut self) -> Result<Vec<PlaybackEndpointObservation>, String>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceDescriptor {
    source_id: SourceId,
    display_name: String,
    source_kind: SourceKind,
    is_available: bool,
    is_default_playback: bool,
    supported_products: Vec<SignalProduct>,
}

impl SourceDescriptor {
    pub(crate) fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    pub(crate) fn display_name(&self) -> &str {
        &self.display_name
    }

    pub(crate) const fn source_kind(&self) -> SourceKind {
        self.source_kind
    }

    pub(crate) const fn is_available(&self) -> bool {
        self.is_available
    }

    pub(crate) const fn is_default_playback(&self) -> bool {
        self.is_default_playback
    }

    pub(crate) fn supported_products(&self) -> &[SignalProduct] {
        &self.supported_products
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PlaybackDiscoverySnapshot {
    identity: IdentitySnapshot,
    sources: Vec<SourceDescriptor>,
    default_playback_binding: Option<PlaybackCaptureBinding>,
}

impl PlaybackDiscoverySnapshot {
    pub(crate) fn namespace(&self) -> &str {
        &self.identity.namespace
    }

    pub(crate) const fn revision(&self) -> u64 {
        self.identity.revision
    }

    pub(crate) fn sources(&self) -> &[SourceDescriptor] {
        &self.sources
    }

    pub(crate) fn to_portable(&self) -> Result<PortableDiscoverySnapshot, DiscoveryContractError> {
        let revision = portable_revision(&self.identity);
        let sources = self
            .sources
            .iter()
            .map(|source| {
                let display_name =
                    (!source.display_name.trim().is_empty()).then(|| source.display_name.clone());
                let availability = if source.is_available {
                    SourceAvailability::Available
                } else {
                    SourceAvailability::Unavailable
                };
                let default_roles = source
                    .is_default_playback
                    .then_some(DefaultSource::Playback)
                    .into_iter()
                    .collect::<Vec<_>>();

                PortableSourceDescriptor::new(
                    source.source_id.clone(),
                    display_name,
                    source.source_kind,
                    availability,
                    source.supported_products.clone(),
                    default_roles,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        PortableDiscoverySnapshot::new(revision, sources)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PlaybackCaptureBinding {
    identity: IdentitySnapshot,
    source_id: SourceId,
    endpoint_id: String,
}

impl PlaybackCaptureBinding {
    pub(crate) fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    pub(crate) fn endpoint_id(&self) -> &str {
        &self.endpoint_id
    }

    #[cfg(test)]
    pub(crate) fn for_test(source_id: SourceId, endpoint_id: impl Into<String>) -> Self {
        Self {
            identity: IdentitySnapshot {
                namespace: "test-namespace".to_string(),
                revision: 1,
            },
            source_id,
            endpoint_id: endpoint_id.into(),
        }
    }
}

#[derive(Debug)]
pub(crate) enum DiscoveryError {
    EndpointEnumeration(String),
    ConflictingDefaultPlaybackEndpoints,
    DefaultPlaybackUnavailable,
    CaptureBindingStale,
    PortableSnapshotStale,
    Registry(RegistryError),
}

impl Display for DiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EndpointEnumeration(message) => formatter.write_str(message),
            Self::ConflictingDefaultPlaybackEndpoints => {
                formatter.write_str("multiple active endpoints claimed the default playback role")
            }
            Self::DefaultPlaybackUnavailable => {
                formatter.write_str("no active endpoint owns the default playback role")
            }
            Self::CaptureBindingStale => formatter
                .write_str("default playback discovery changed before capture startup completed"),
            Self::PortableSnapshotStale => {
                formatter.write_str("portable discovery snapshot is stale")
            }
            Self::Registry(error) => error.fmt(formatter),
        }
    }
}

impl Error for DiscoveryError {}

impl From<RegistryError> for DiscoveryError {
    fn from(error: RegistryError) -> Self {
        Self::Registry(error)
    }
}

pub(crate) struct PlaybackDiscovery<S: PlaybackEndpointSource> {
    endpoint_source: S,
    registry: IdentityRegistry,
    last_snapshot: Option<PlaybackDiscoverySnapshot>,
}

impl<S: PlaybackEndpointSource> PlaybackDiscovery<S> {
    pub(crate) fn new(
        storage_directory: impl AsRef<Path>,
        endpoint_source: S,
    ) -> Result<Self, DiscoveryError> {
        Ok(Self {
            endpoint_source,
            registry: IdentityRegistry::new(storage_directory)?,
            last_snapshot: None,
        })
    }

    pub(crate) fn endpoint_source_mut(&mut self) -> &mut S {
        &mut self.endpoint_source
    }

    pub(crate) fn refresh(&mut self) -> Result<PlaybackDiscoverySnapshot, DiscoveryError> {
        let observations = self
            .endpoint_source
            .enumerate_playback_endpoints()
            .map_err(DiscoveryError::EndpointEnumeration)?;
        let active: Vec<_> = observations
            .into_iter()
            .filter(|observation| observation.state == PlaybackEndpointState::Active)
            .collect();

        if active
            .iter()
            .filter(|observation| observation.is_default_playback)
            .count()
            > 1
        {
            return Err(DiscoveryError::ConflictingDefaultPlaybackEndpoints);
        }

        let continuity_counts = stable_continuity_counts(&active);
        let mut ambiguous_duplicates = BTreeSet::new();
        let mut registry_observations = Vec::new();
        let mut describable = Vec::new();

        for observation in active {
            match &observation.continuity {
                ObservedContinuity::Stable {
                    backend_key,
                    continuity_token,
                } => {
                    let key = (backend_key.clone(), continuity_token.clone());
                    if continuity_counts.get(&key).copied().unwrap_or_default() == 1 {
                        registry_observations.push(SourceObservation {
                            display_name: observation.display_name.clone(),
                            continuity: observation.continuity.clone(),
                        });
                        describable.push(observation);
                    } else if ambiguous_duplicates.insert(key.clone()) {
                        registry_observations.push(SourceObservation {
                            display_name: observation.display_name,
                            continuity: ObservedContinuity::Ambiguous(vec![ContinuityEvidence {
                                backend_key: key.0,
                                continuity_token: key.1,
                            }]),
                        });
                    }
                }
                ObservedContinuity::Ambiguous(_) | ObservedContinuity::Unknown => {
                    registry_observations.push(SourceObservation {
                        display_name: observation.display_name,
                        continuity: observation.continuity,
                    });
                }
            }
        }

        let revision_before_refresh = self.registry.revision();
        let identity = self.registry.apply_observations(&registry_observations)?;
        let default_playback_observation = describable
            .iter()
            .find(|observation| observation.is_default_playback);
        let default_playback_source_id = default_playback_observation.and_then(|observation| {
            self.registry
                .source_id_for_observation(&observation.continuity)
        });
        let sources = self
            .registry
            .known_sources()
            .into_iter()
            .map(|known| SourceDescriptor {
                is_default_playback: default_playback_source_id
                    .as_ref()
                    .is_some_and(|source_id| source_id == &known.source_id),
                source_id: known.source_id,
                display_name: known.display_name,
                source_kind: SourceKind::Playback,
                is_available: known.is_available,
                supported_products: vec![SignalProduct::Waveform],
            })
            .collect();

        let default_playback_binding = default_playback_observation.and_then(|observation| {
            let ObservedContinuity::Stable { backend_key, .. } = &observation.continuity else {
                return None;
            };
            default_playback_source_id
                .as_ref()
                .map(|source_id| PlaybackCaptureBinding {
                    identity: identity.clone(),
                    source_id: source_id.clone(),
                    endpoint_id: backend_key.clone(),
                })
        });
        let mut snapshot = PlaybackDiscoverySnapshot {
            identity,
            sources,
            default_playback_binding,
        };
        let reopened_without_registry_change =
            self.last_snapshot.is_none() && snapshot.identity.revision == revision_before_refresh;
        let descriptor_change_without_registry_change =
            self.last_snapshot.as_ref().is_some_and(|previous| {
                previous.sources != snapshot.sources
                    && previous.identity.revision == snapshot.identity.revision
            });
        if reopened_without_registry_change || descriptor_change_without_registry_change {
            snapshot.identity = self.registry.advance_revision()?;
            if let Some(binding) = snapshot.default_playback_binding.as_mut() {
                binding.identity = snapshot.identity.clone();
            }
        }
        self.last_snapshot = Some(snapshot.clone());
        Ok(snapshot)
    }

    pub(crate) fn resolve_default_playback(
        &self,
        snapshot: &PlaybackDiscoverySnapshot,
    ) -> Result<SourceId, DiscoveryError> {
        self.registry.validate_snapshot(&snapshot.identity)?;
        snapshot
            .sources
            .iter()
            .find(|source| source.is_default_playback)
            .map(|source| source.source_id.clone())
            .ok_or(DiscoveryError::DefaultPlaybackUnavailable)
    }

    pub(crate) fn refresh_default_playback_capture(
        &mut self,
    ) -> Result<PlaybackCaptureBinding, DiscoveryError> {
        let snapshot = self.refresh()?;
        self.resolve_default_playback_capture(&snapshot)
    }

    pub(crate) fn resolve_default_playback_capture(
        &self,
        snapshot: &PlaybackDiscoverySnapshot,
    ) -> Result<PlaybackCaptureBinding, DiscoveryError> {
        self.registry.validate_snapshot(&snapshot.identity)?;
        snapshot
            .default_playback_binding
            .clone()
            .ok_or(DiscoveryError::DefaultPlaybackUnavailable)
    }

    pub(crate) fn revalidate_default_playback_capture(
        &mut self,
        binding: &PlaybackCaptureBinding,
    ) -> Result<(), DiscoveryError> {
        let current = self.refresh_default_playback_capture()?;
        if &current != binding {
            return Err(DiscoveryError::CaptureBindingStale);
        }
        Ok(())
    }

    pub(crate) fn resolve_explicit(
        &self,
        source_id: &SourceId,
        snapshot: &PlaybackDiscoverySnapshot,
    ) -> Result<SourceId, DiscoveryError> {
        match self.registry.resolve(source_id, &snapshot.identity)? {
            SourceResolution::Live(source_id) => Ok(source_id),
            SourceResolution::Absent | SourceResolution::Retired | SourceResolution::Unknown => {
                unreachable!("registry returns typed errors")
            }
        }
    }

    pub(crate) fn resolve_explicit_at_revision(
        &self,
        source_id: &SourceId,
        revision: &DiscoveryRevision,
    ) -> Result<SourceId, DiscoveryError> {
        let current = self.registry.snapshot();
        if &portable_revision(&current) != revision {
            return Err(DiscoveryError::PortableSnapshotStale);
        }

        match self.registry.resolve(source_id, &current)? {
            SourceResolution::Live(source_id) => Ok(source_id),
            SourceResolution::Absent | SourceResolution::Retired | SourceResolution::Unknown => {
                unreachable!("registry returns typed errors")
            }
        }
    }
}

fn portable_revision(identity: &IdentitySnapshot) -> DiscoveryRevision {
    DiscoveryRevision::new(format!(
        "{}|{}|{}",
        identity.namespace.len(),
        identity.namespace,
        identity.revision
    ))
    .expect("a provider discovery revision is always non-empty")
}

fn stable_continuity_counts(
    observations: &[PlaybackEndpointObservation],
) -> BTreeMap<(String, String), usize> {
    let mut counts = BTreeMap::new();
    for observation in observations {
        if let ObservedContinuity::Stable {
            backend_key,
            continuity_token,
        } = &observation.continuity
        {
            *counts
                .entry((backend_key.clone(), continuity_token.clone()))
                .or_insert(0) += 1;
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct FakeEndpointSource {
        observations: Vec<PlaybackEndpointObservation>,
    }

    impl PlaybackEndpointSource for FakeEndpointSource {
        fn enumerate_playback_endpoints(
            &mut self,
        ) -> Result<Vec<PlaybackEndpointObservation>, String> {
            Ok(self.observations.clone())
        }
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let mut path = env::temp_dir();
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            path.push(format!(
                "resonance-discovery-test-{}-{nanos}",
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

    fn stable_endpoint(
        backend_key: &str,
        continuity_token: &str,
        display_name: &str,
        state: PlaybackEndpointState,
        is_default_playback: bool,
    ) -> PlaybackEndpointObservation {
        PlaybackEndpointObservation {
            display_name: display_name.to_string(),
            state,
            continuity: ObservedContinuity::Stable {
                backend_key: backend_key.to_string(),
                continuity_token: continuity_token.to_string(),
            },
            is_default_playback,
        }
    }

    fn source_id_named(snapshot: &PlaybackDiscoverySnapshot, name: &str) -> SourceId {
        snapshot
            .sources()
            .iter()
            .find(|source| source.display_name() == name)
            .unwrap()
            .source_id()
            .clone()
    }

    #[test]
    fn duplicate_names_remain_distinct_registry_sources() {
        let directory = TestDirectory::new();
        let source = FakeEndpointSource {
            observations: vec![
                stable_endpoint("a", "v1", "Headset", PlaybackEndpointState::Active, true),
                stable_endpoint("b", "v1", "Headset", PlaybackEndpointState::Active, false),
            ],
        };
        let mut discovery = PlaybackDiscovery::new(&directory.0, source).unwrap();

        let snapshot = discovery.refresh().unwrap();
        let portable = snapshot.to_portable().unwrap();

        assert_eq!(snapshot.sources().len(), 2);
        assert_ne!(
            snapshot.sources()[0].source_id(),
            snapshot.sources()[1].source_id()
        );
        assert_eq!(portable.sources().len(), 2);
        assert_eq!(portable.sources()[0].display_name(), Some("Headset"));
        assert_eq!(portable.sources()[1].display_name(), Some("Headset"));
        assert_ne!(
            portable.sources()[0].source_id(),
            portable.sources()[1].source_id()
        );
    }

    #[test]
    fn metadata_rename_preserves_source_id() {
        let directory = TestDirectory::new();
        let mut discovery = PlaybackDiscovery::new(
            &directory.0,
            FakeEndpointSource {
                observations: vec![stable_endpoint(
                    "a",
                    "v1",
                    "Headset",
                    PlaybackEndpointState::Active,
                    true,
                )],
            },
        )
        .unwrap();
        let before = discovery.refresh().unwrap();
        let source_id = source_id_named(&before, "Headset");

        discovery.endpoint_source_mut().observations[0].display_name = "Renamed".to_string();
        let after = discovery.refresh().unwrap();
        let portable = after.to_portable().unwrap();

        assert_eq!(source_id, source_id_named(&after, "Renamed"));
        assert!(after.revision() > before.revision());
        assert_eq!(portable.sources()[0].source_id(), &source_id);
        assert_eq!(portable.sources()[0].display_name(), Some("Renamed"));
    }

    #[test]
    fn default_role_movement_changes_resolution_not_identity() {
        let directory = TestDirectory::new();
        let mut discovery = PlaybackDiscovery::new(
            &directory.0,
            FakeEndpointSource {
                observations: vec![
                    stable_endpoint("a", "v1", "A", PlaybackEndpointState::Active, true),
                    stable_endpoint("b", "v1", "B", PlaybackEndpointState::Active, false),
                ],
            },
        )
        .unwrap();
        let before = discovery.refresh().unwrap();
        let a = source_id_named(&before, "A");
        let b = source_id_named(&before, "B");
        assert_eq!(discovery.resolve_default_playback(&before).unwrap(), a);

        discovery.endpoint_source_mut().observations[0].is_default_playback = false;
        discovery.endpoint_source_mut().observations[1].is_default_playback = true;
        let after = discovery.refresh().unwrap();
        let portable = after.to_portable().unwrap();

        assert_eq!(source_id_named(&after, "A"), a);
        assert_eq!(source_id_named(&after, "B"), b);
        assert_eq!(discovery.resolve_default_playback(&after).unwrap(), b);
        assert!(after.revision() > before.revision());
        assert!(!portable
            .sources()
            .iter()
            .find(|source| source.source_id() == &a)
            .unwrap()
            .is_default_playback());
        assert!(portable
            .sources()
            .iter()
            .find(|source| source.source_id() == &b)
            .unwrap()
            .is_default_playback());
    }

    #[test]
    fn capture_binding_is_attempt_scoped_and_rejects_role_movement() {
        let directory = TestDirectory::new();
        let mut discovery = PlaybackDiscovery::new(
            &directory.0,
            FakeEndpointSource {
                observations: vec![
                    stable_endpoint(
                        "endpoint-a",
                        "v1",
                        "Same",
                        PlaybackEndpointState::Active,
                        true,
                    ),
                    stable_endpoint(
                        "endpoint-b",
                        "v1",
                        "Same",
                        PlaybackEndpointState::Active,
                        false,
                    ),
                ],
            },
        )
        .unwrap();

        let before = discovery.refresh().unwrap();
        let source_a = before.sources()[0].source_id().clone();
        let source_b = before.sources()[1].source_id().clone();
        let attempt_a = discovery.resolve_default_playback_capture(&before).unwrap();
        assert_eq!(attempt_a.source_id(), &source_a);
        assert_eq!(attempt_a.endpoint_id(), "endpoint-a");

        discovery.endpoint_source_mut().observations[0].is_default_playback = false;
        discovery.endpoint_source_mut().observations[1].is_default_playback = true;
        assert!(matches!(
            discovery.revalidate_default_playback_capture(&attempt_a),
            Err(DiscoveryError::CaptureBindingStale)
        ));

        let current = discovery.refresh_default_playback_capture().unwrap();
        assert_eq!(current.source_id(), &source_b);
        assert_eq!(current.endpoint_id(), "endpoint-b");
        let current_snapshot = discovery.refresh().unwrap();
        assert_eq!(current_snapshot.sources()[0].source_id(), &source_a);
        assert_eq!(current_snapshot.sources()[1].source_id(), &source_b);
    }

    #[test]
    fn capture_binding_fails_closed_without_a_default_and_is_never_synthetic() {
        let directory = TestDirectory::new();
        let mut discovery = PlaybackDiscovery::new(
            &directory.0,
            FakeEndpointSource {
                observations: vec![stable_endpoint(
                    "endpoint-a",
                    "v1",
                    "A",
                    PlaybackEndpointState::Active,
                    false,
                )],
            },
        )
        .unwrap();

        assert!(matches!(
            discovery.refresh_default_playback_capture(),
            Err(DiscoveryError::DefaultPlaybackUnavailable)
        ));

        discovery.endpoint_source_mut().observations[0].is_default_playback = true;
        let binding = discovery.refresh_default_playback_capture().unwrap();
        assert_ne!(binding.source_id().as_str(), "default-playback");
        assert_eq!(binding.endpoint_id(), "endpoint-a");
    }

    #[test]
    fn only_active_playback_endpoints_are_selectable() {
        let directory = TestDirectory::new();
        let source = FakeEndpointSource {
            observations: vec![
                stable_endpoint("a", "v1", "Active", PlaybackEndpointState::Active, true),
                stable_endpoint(
                    "b",
                    "v1",
                    "Disabled",
                    PlaybackEndpointState::Disabled,
                    false,
                ),
                stable_endpoint(
                    "c",
                    "v1",
                    "Missing",
                    PlaybackEndpointState::NotPresent,
                    false,
                ),
                stable_endpoint(
                    "d",
                    "v1",
                    "Unplugged",
                    PlaybackEndpointState::Unplugged,
                    false,
                ),
            ],
        };
        let mut discovery = PlaybackDiscovery::new(&directory.0, source).unwrap();

        let snapshot = discovery.refresh().unwrap();

        assert_eq!(snapshot.sources().len(), 1);
        let descriptor = &snapshot.sources()[0];
        assert_eq!(descriptor.display_name(), "Active");
        assert_eq!(descriptor.source_kind(), SourceKind::Playback);
        assert!(descriptor.is_available());
        assert_eq!(descriptor.supported_products(), &[SignalProduct::Waveform]);
    }

    #[test]
    fn temporary_disappearance_and_proven_return_restore_source_id() {
        let directory = TestDirectory::new();
        let mut discovery = PlaybackDiscovery::new(
            &directory.0,
            FakeEndpointSource {
                observations: vec![stable_endpoint(
                    "a",
                    "v1",
                    "A",
                    PlaybackEndpointState::Active,
                    true,
                )],
            },
        )
        .unwrap();
        let present = discovery.refresh().unwrap();
        let source_id = source_id_named(&present, "A");

        discovery.endpoint_source_mut().observations.clear();
        let absent = discovery.refresh().unwrap();
        let portable_absent = absent.to_portable().unwrap();
        assert_eq!(absent.sources().len(), 1);
        assert!(!absent.sources()[0].is_available());
        assert!(!absent.sources()[0].is_default_playback());
        assert_eq!(portable_absent.sources().len(), 1);
        assert_eq!(portable_absent.sources()[0].source_id(), &source_id);
        assert_eq!(
            portable_absent.sources()[0].availability(),
            SourceAvailability::Unavailable
        );
        assert!(matches!(
            discovery.resolve_explicit(&source_id, &absent),
            Err(DiscoveryError::Registry(RegistryError::SourceUnavailable))
        ));

        discovery
            .endpoint_source_mut()
            .observations
            .push(stable_endpoint(
                "a",
                "v1",
                "A",
                PlaybackEndpointState::Active,
                true,
            ));
        let returned = discovery.refresh().unwrap();
        assert_eq!(source_id_named(&returned, "A"), source_id);
    }

    #[test]
    fn native_key_reuse_retires_old_identity() {
        let directory = TestDirectory::new();
        let mut discovery = PlaybackDiscovery::new(
            &directory.0,
            FakeEndpointSource {
                observations: vec![stable_endpoint(
                    "a",
                    "old",
                    "A",
                    PlaybackEndpointState::Active,
                    true,
                )],
            },
        )
        .unwrap();
        let before = discovery.refresh().unwrap();
        let old = source_id_named(&before, "A");

        discovery.endpoint_source_mut().observations[0] =
            stable_endpoint("a", "new", "A", PlaybackEndpointState::Active, true);
        let after = discovery.refresh().unwrap();

        assert_ne!(source_id_named(&after, "A"), old);
        assert!(matches!(
            discovery.resolve_explicit(&old, &after),
            Err(DiscoveryError::Registry(RegistryError::SourceRetired))
        ));
    }

    #[test]
    fn ambiguous_duplicate_continuity_is_not_selectable() {
        let directory = TestDirectory::new();
        let source = FakeEndpointSource {
            observations: vec![
                stable_endpoint("a", "v1", "One", PlaybackEndpointState::Active, true),
                stable_endpoint("a", "v1", "Two", PlaybackEndpointState::Active, false),
            ],
        };
        let mut discovery = PlaybackDiscovery::new(&directory.0, source).unwrap();

        let snapshot = discovery.refresh().unwrap();

        assert!(snapshot.sources().is_empty());
        assert!(matches!(
            discovery.resolve_default_playback(&snapshot),
            Err(DiscoveryError::DefaultPlaybackUnavailable)
        ));
    }

    #[test]
    fn registry_reopen_preserves_namespace_and_source_ids() {
        let directory = TestDirectory::new();
        let observation = stable_endpoint("a", "v1", "A", PlaybackEndpointState::Active, true);
        let before = {
            let mut discovery = PlaybackDiscovery::new(
                &directory.0,
                FakeEndpointSource {
                    observations: vec![observation.clone()],
                },
            )
            .unwrap();
            discovery.refresh().unwrap()
        };

        let mut reopened = PlaybackDiscovery::new(
            &directory.0,
            FakeEndpointSource {
                observations: vec![observation],
            },
        )
        .unwrap();
        let after = reopened.refresh().unwrap();

        assert_eq!(before.namespace(), after.namespace());
        let source_id = source_id_named(&before, "A");
        assert_eq!(source_id, source_id_named(&after, "A"));
        assert!(after.revision() > before.revision());
        assert!(matches!(
            reopened.resolve_explicit(&source_id, &before),
            Err(DiscoveryError::Registry(
                RegistryError::SnapshotStale { .. }
            ))
        ));
    }

    #[test]
    fn stale_snapshot_is_rejected_after_newer_refresh() {
        let directory = TestDirectory::new();
        let mut discovery = PlaybackDiscovery::new(
            &directory.0,
            FakeEndpointSource {
                observations: vec![stable_endpoint(
                    "a",
                    "v1",
                    "A",
                    PlaybackEndpointState::Active,
                    true,
                )],
            },
        )
        .unwrap();
        let stale = discovery.refresh().unwrap();
        let source_id = source_id_named(&stale, "A");
        let portable_stale = stale.to_portable().unwrap();
        discovery.endpoint_source_mut().observations[0].display_name = "Renamed".to_string();
        let current = discovery.refresh().unwrap();
        let portable_current = current.to_portable().unwrap();

        assert!(matches!(
            discovery.resolve_explicit(&source_id, &stale),
            Err(DiscoveryError::Registry(
                RegistryError::SnapshotStale { .. }
            ))
        ));
        assert_ne!(portable_stale.revision(), portable_current.revision());
        assert!(matches!(
            discovery.resolve_explicit_at_revision(&source_id, portable_stale.revision()),
            Err(DiscoveryError::PortableSnapshotStale)
        ));
        assert_eq!(
            discovery
                .resolve_explicit_at_revision(&source_id, portable_current.revision())
                .unwrap(),
            source_id
        );
    }

    #[test]
    fn unavailable_explicit_source_never_substitutes_default() {
        let directory = TestDirectory::new();
        let mut discovery = PlaybackDiscovery::new(
            &directory.0,
            FakeEndpointSource {
                observations: vec![
                    stable_endpoint("a", "v1", "A", PlaybackEndpointState::Active, false),
                    stable_endpoint("b", "v1", "B", PlaybackEndpointState::Active, true),
                ],
            },
        )
        .unwrap();
        let before = discovery.refresh().unwrap();
        let a = source_id_named(&before, "A");
        let b = source_id_named(&before, "B");

        discovery
            .endpoint_source_mut()
            .observations
            .retain(|observation| observation.display_name == "B");
        let after = discovery.refresh().unwrap();

        assert_eq!(discovery.resolve_default_playback(&after).unwrap(), b);
        assert!(matches!(
            discovery.resolve_explicit(&a, &after),
            Err(DiscoveryError::Registry(RegistryError::SourceUnavailable))
        ));
    }

    #[test]
    fn no_op_refresh_keeps_revision_stable() {
        let directory = TestDirectory::new();
        let mut discovery = PlaybackDiscovery::new(
            &directory.0,
            FakeEndpointSource {
                observations: vec![stable_endpoint(
                    "a",
                    "v1",
                    "A",
                    PlaybackEndpointState::Active,
                    true,
                )],
            },
        )
        .unwrap();

        let first = discovery.refresh().unwrap();
        let second = discovery.refresh().unwrap();

        assert_eq!(first.revision(), second.revision());
        assert_eq!(
            first.to_portable().unwrap().revision(),
            second.to_portable().unwrap().revision()
        );
    }

    #[test]
    fn portable_snapshot_is_sanitized_owned_and_capability_conservative() {
        let directory = TestDirectory::new();
        let native_endpoint_id = "{private-wasapi-endpoint-id}";
        let continuity_token = "private-continuity-evidence";
        let mut discovery = PlaybackDiscovery::new(
            &directory.0,
            FakeEndpointSource {
                observations: vec![stable_endpoint(
                    native_endpoint_id,
                    continuity_token,
                    "Original",
                    PlaybackEndpointState::Active,
                    true,
                )],
            },
        )
        .unwrap();

        let private_before = discovery.refresh().unwrap();
        let portable_before = private_before.to_portable().unwrap();
        discovery.endpoint_source_mut().observations[0].display_name = "Renamed".to_string();
        let private_after = discovery.refresh().unwrap();
        let portable_after = private_after.to_portable().unwrap();

        assert_eq!(
            portable_before.sources()[0].display_name(),
            Some("Original")
        );
        assert_eq!(portable_after.sources()[0].display_name(), Some("Renamed"));
        assert_eq!(
            portable_before.sources()[0].source_id(),
            portable_after.sources()[0].source_id()
        );
        assert_ne!(portable_before.revision(), portable_after.revision());
        assert_eq!(
            portable_after.sources()[0].supported_products(),
            &[SignalProduct::Waveform]
        );
        assert_eq!(portable_after.sources()[0].kind(), SourceKind::Playback);
        assert_eq!(
            portable_after.sources()[0].availability(),
            SourceAvailability::Available
        );
        assert!(portable_after.sources()[0].is_default_playback());

        let consumer_debug = format!("{portable_after:?}");
        assert!(!consumer_debug.contains(native_endpoint_id));
        assert!(!consumer_debug.contains(continuity_token));
    }
}
