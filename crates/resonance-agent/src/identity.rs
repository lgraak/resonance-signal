use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Display};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use resonance_api::contract::SourceId;

const CURRENT_SCHEMA_VERSION: u32 = 1;
const CURRENT_EVIDENCE_SCHEMA_VERSION: u32 = 1;
const REGISTRY_FILENAME: &str = "source-identity-registry.json";
const REGISTRY_CANDIDATE_SUFFIX: &str = "candidate";
const REGISTRY_BACKUP_SUFFIX: &str = "backup";

#[derive(Clone, Debug, Eq, PartialEq)]
struct PersistedRegistry {
    schema_version: u32,
    evidence_schema_version: u32,
    namespace: String,
    revision: u64,
    next_source_sequence: u64,
    entries: Vec<PersistedSourceRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PersistedSourceRecord {
    source_id: String,
    continuity: ContinuityEvidence,
    display_name: String,
    state: PersistedSourceState,
    retired_reason: Option<PersistedRetiredReason>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ContinuityEvidence {
    pub(crate) backend_key: String,
    pub(crate) continuity_token: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PersistedSourceState {
    Live,
    Absent,
    Retired,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PersistedRetiredReason {
    NativeKeyReuse,
    AmbiguousReturn,
    MigrationCorruption,
    NamespaceReset,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SourceState {
    Live,
    Absent,
    Retired(RetiredReason),
}

impl SourceState {
    fn is_live(&self) -> bool {
        matches!(self, Self::Live)
    }

    fn is_retired(&self) -> bool {
        matches!(self, Self::Retired(_))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RetiredReason {
    NativeKeyReuse,
    AmbiguousReturn,
    MigrationCorruption,
    NamespaceReset,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceRecord {
    id: SourceId,
    continuity: ContinuityEvidence,
    display_name: String,
    state: SourceState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceObservation {
    pub(crate) display_name: String,
    pub(crate) continuity: ObservedContinuity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ObservedContinuity {
    Stable {
        backend_key: String,
        continuity_token: String,
    },
    Ambiguous(Vec<ContinuityEvidence>),
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiscoverySnapshot {
    pub(crate) namespace: String,
    pub(crate) revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SourceResolution {
    Live(SourceId),
    Absent,
    Retired,
    Unknown,
}

#[derive(Debug)]
pub(crate) enum RegistryError {
    Io(io::Error),
    UnsupportedSchema { expected: u32, observed: u32 },
    CorruptRegistry,
    SnapshotStale { expected: u64, observed: u64 },
    UnknownSource,
    SourceUnavailable,
    SourceRetired,
    SourceIdAllocationFailed,
    MigrationFailed,
}

impl Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::UnsupportedSchema { expected, observed } => write!(
                formatter,
                "unsupported schema version: expected {expected}, observed {observed}",
            ),
            Self::CorruptRegistry => formatter.write_str("registry image is corrupt"),
            Self::SnapshotStale { expected, observed } => write!(
                formatter,
                "snapshot is stale: expected revision {expected}, observed {observed}",
            ),
            Self::UnknownSource => formatter.write_str("requested source is unknown in namespace"),
            Self::SourceUnavailable => {
                formatter.write_str("requested source is currently unavailable")
            }
            Self::SourceRetired => formatter.write_str("requested source is retired"),
            Self::SourceIdAllocationFailed => {
                formatter.write_str("failed to allocate a new source id")
            }
            Self::MigrationFailed => formatter.write_str("registry migration failed"),
        }
    }
}

impl From<io::Error> for RegistryError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub(crate) trait SourceIdAllocator {
    fn next_source_id(&mut self, namespace: &str, sequence: u64) -> String;
}

#[derive(Clone)]
pub(crate) struct CounterSourceIdAllocator;

impl SourceIdAllocator for CounterSourceIdAllocator {
    fn next_source_id(&mut self, namespace: &str, sequence: u64) -> String {
        format!("id-{namespace}-{sequence}")
    }
}

#[derive(Debug)]
pub(crate) struct IdentityRegistry<A: SourceIdAllocator = CounterSourceIdAllocator> {
    namespace: String,
    revision: u64,
    next_source_sequence: u64,
    storage_directory: PathBuf,
    allocator: A,
    entries: BTreeMap<String, SourceRecord>,
}

impl IdentityRegistry<CounterSourceIdAllocator> {
    pub(crate) fn new(storage_directory: impl AsRef<Path>) -> Result<Self, RegistryError> {
        Self::with_allocator(storage_directory, CounterSourceIdAllocator)
    }
}

impl<A: SourceIdAllocator> IdentityRegistry<A> {
    pub(crate) fn with_allocator(
        storage_directory: impl AsRef<Path>,
        allocator: A,
    ) -> Result<Self, RegistryError> {
        Self::load_or_init(storage_directory.as_ref().to_path_buf(), allocator)
    }

    fn load_or_init(storage_directory: PathBuf, allocator: A) -> Result<Self, RegistryError> {
        fs::create_dir_all(&storage_directory)?;
        let layout = RegistryStorageLayout::new(&storage_directory);

        let image = match load_registry(&layout.current) {
            Ok(image) => image,
            Err(_) => match load_registry(&layout.backup) {
                Ok(image) => image,
                Err(_) => return Self::new_blank(storage_directory, allocator),
            },
        };

        Self::from_persisted(storage_directory, image, allocator)
    }

    fn from_persisted(
        storage_directory: PathBuf,
        image: PersistedRegistry,
        allocator: A,
    ) -> Result<Self, RegistryError> {
        let image = image.into_current_schema()?;

        let mut entries = BTreeMap::new();
        let mut continuity_index = BTreeSet::new();

        for persisted in image.entries {
            let source_id =
                SourceId::new(persisted.source_id).map_err(|_| RegistryError::CorruptRegistry)?;
            if entries.contains_key(source_id.as_str()) {
                return Err(RegistryError::CorruptRegistry);
            }
            if persisted.continuity.backend_key.is_empty()
                || persisted.continuity.continuity_token.is_empty()
            {
                return Err(RegistryError::CorruptRegistry);
            }
            let continuity_key = (
                persisted.continuity.backend_key.clone(),
                persisted.continuity.continuity_token.clone(),
            );
            if !continuity_index.insert(continuity_key) {
                return Err(RegistryError::CorruptRegistry);
            }
            let state = match persisted.state {
                PersistedSourceState::Live => SourceState::Live,
                PersistedSourceState::Absent => SourceState::Absent,
                PersistedSourceState::Retired => {
                    let reason = persisted
                        .retired_reason
                        .ok_or(RegistryError::CorruptRegistry)?;
                    SourceState::Retired(reason.into())
                }
            };
            entries.insert(
                source_id.as_str().to_string(),
                SourceRecord {
                    id: source_id,
                    continuity: persisted.continuity,
                    display_name: persisted.display_name,
                    state,
                },
            );
        }

        let mut next_source_sequence = image.next_source_sequence;
        for entry in entries.values() {
            if let Some(parsed) = parse_sequence_from_id(entry.id.as_str()) {
                if parsed > next_source_sequence {
                    next_source_sequence = parsed;
                }
            }
        }

        Ok(Self {
            namespace: image.namespace,
            revision: image.revision,
            next_source_sequence,
            storage_directory,
            allocator,
            entries,
        })
    }

    fn new_blank(storage_directory: PathBuf, allocator: A) -> Result<Self, RegistryError> {
        let registry = Self {
            namespace: generate_namespace(),
            revision: 1,
            next_source_sequence: 0,
            storage_directory,
            allocator,
            entries: BTreeMap::new(),
        };
        registry.persist()?;
        Ok(registry)
    }

    pub(crate) fn namespace(&self) -> &str {
        &self.namespace
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn snapshot(&self) -> DiscoverySnapshot {
        DiscoverySnapshot {
            namespace: self.namespace.clone(),
            revision: self.revision,
        }
    }

    pub(crate) fn validate_snapshot(
        &self,
        snapshot: &DiscoverySnapshot,
    ) -> Result<(), RegistryError> {
        if snapshot.namespace != self.namespace || snapshot.revision != self.revision {
            return Err(RegistryError::SnapshotStale {
                expected: self.revision,
                observed: snapshot.revision,
            });
        }
        Ok(())
    }

    pub(crate) fn source_id_for_continuity(
        &self,
        continuity: &ContinuityEvidence,
    ) -> Option<SourceId> {
        self.entries
            .values()
            .find(|entry| !entry.state.is_retired() && entry.continuity == *continuity)
            .map(|entry| entry.id.clone())
    }

    pub(crate) fn source_id_for_observation(
        &self,
        continuity: &ObservedContinuity,
    ) -> Option<SourceId> {
        match continuity {
            ObservedContinuity::Stable {
                backend_key,
                continuity_token,
            } => self.source_id_for_continuity(&ContinuityEvidence {
                backend_key: backend_key.clone(),
                continuity_token: continuity_token.clone(),
            }),
            ObservedContinuity::Ambiguous(_) | ObservedContinuity::Unknown => None,
        }
    }

    pub(crate) fn resolve(
        &self,
        source_id: &SourceId,
        snapshot: &DiscoverySnapshot,
    ) -> Result<SourceResolution, RegistryError> {
        self.validate_snapshot(snapshot)?;

        match self.entries.get(source_id.as_str()) {
            None => Err(RegistryError::UnknownSource),
            Some(entry) => match &entry.state {
                SourceState::Live => Ok(SourceResolution::Live(entry.id.clone())),
                SourceState::Absent => Err(RegistryError::SourceUnavailable),
                SourceState::Retired(_) => Err(RegistryError::SourceRetired),
            },
        }
    }

    pub(crate) fn apply_observations(
        &mut self,
        observations: &[SourceObservation],
    ) -> Result<DiscoverySnapshot, RegistryError> {
        let mut changed = false;
        let observed_continuities: Vec<(String, String)> = observations
            .iter()
            .filter_map(|observation| match &observation.continuity {
                ObservedContinuity::Stable {
                    backend_key,
                    continuity_token,
                } => Some((backend_key.clone(), continuity_token.clone())),
                _ => None,
            })
            .collect();

        for entry in self.entries.values_mut() {
            let continuity_key = (
                entry.continuity.backend_key.clone(),
                entry.continuity.continuity_token.clone(),
            );
            if entry.state.is_live() && !observed_continuities.contains(&continuity_key) {
                entry.state = SourceState::Absent;
                changed = true;
            }
        }

        for observation in observations {
            match &observation.continuity {
                ObservedContinuity::Stable {
                    backend_key,
                    continuity_token,
                } => {
                    let continuity = ContinuityEvidence {
                        backend_key: backend_key.clone(),
                        continuity_token: continuity_token.clone(),
                    };
                    if let Some(existing_id) = self.find_live_source_id(&continuity) {
                        let entry = self
                            .entries
                            .get_mut(existing_id.as_str())
                            .expect("existing source exists in map");
                        if !entry.state.is_live() {
                            entry.state = SourceState::Live;
                            changed = true;
                        }
                        if entry.display_name != observation.display_name {
                            entry.display_name = observation.display_name.clone();
                            changed = true;
                        }
                        continue;
                    }

                    let reuse_candidates: Vec<String> = self
                        .entries
                        .iter()
                        .filter(|(_, entry)| {
                            !entry.state.is_retired()
                                && entry.continuity != continuity
                                && entry.continuity.backend_key == continuity.backend_key
                                && !observed_continuities.contains(&(
                                    entry.continuity.backend_key.clone(),
                                    entry.continuity.continuity_token.clone(),
                                ))
                        })
                        .map(|(id, _)| id.clone())
                        .collect();
                    if reuse_candidates.len() == 1 {
                        self.retire(&reuse_candidates[0], RetiredReason::NativeKeyReuse);
                    }

                    let source_id = self.allocate_source_id()?;
                    self.entries.insert(
                        source_id.as_str().to_string(),
                        SourceRecord {
                            id: source_id,
                            continuity,
                            display_name: observation.display_name.clone(),
                            state: SourceState::Live,
                        },
                    );
                    changed = true;
                }
                ObservedContinuity::Ambiguous(candidates) => {
                    let mut to_retire = Vec::new();
                    for candidate in candidates {
                        for (id, entry) in self.entries.iter() {
                            if !entry.state.is_retired() && entry.continuity == *candidate {
                                to_retire.push(id.clone());
                            }
                        }
                    }
                    to_retire.sort_unstable();
                    to_retire.dedup();
                    let retired_count = to_retire.len();
                    for id in to_retire {
                        self.retire(&id, RetiredReason::AmbiguousReturn);
                    }
                    if retired_count > 0 {
                        changed = true;
                    }
                }
                ObservedContinuity::Unknown => {}
            }
        }

        if changed {
            self.revision = self.revision.saturating_add(1);
            self.persist()?;
        }

        Ok(self.snapshot())
    }

    pub(crate) fn advance_revision(&mut self) -> Result<DiscoverySnapshot, RegistryError> {
        self.revision = self.revision.saturating_add(1);
        self.persist()?;
        Ok(self.snapshot())
    }

    pub(crate) fn reset_namespace(&mut self) -> Result<(), RegistryError> {
        self.namespace = generate_namespace();
        self.revision = self.revision.saturating_add(1);
        self.next_source_sequence = 0;
        self.entries.clear();
        self.persist()?;
        Ok(())
    }

    fn find_live_source_id(&self, continuity: &ContinuityEvidence) -> Option<SourceId> {
        self.entries
            .values()
            .find(|entry| !entry.state.is_retired() && entry.continuity == *continuity)
            .map(|entry| entry.id.clone())
    }

    fn retire(&mut self, source_id: &str, reason: RetiredReason) {
        if let Some(entry) = self.entries.get_mut(source_id) {
            if !entry.state.is_retired() {
                entry.state = SourceState::Retired(reason);
            }
        }
    }

    fn allocate_source_id(&mut self) -> Result<SourceId, RegistryError> {
        for _ in 0..4096 {
            self.next_source_sequence = self.next_source_sequence.saturating_add(1);
            let candidate = self
                .allocator
                .next_source_id(&self.namespace, self.next_source_sequence);
            if candidate.is_empty() || self.entries.contains_key(&candidate) {
                continue;
            }
            return SourceId::new(candidate).map_err(|_| RegistryError::SourceIdAllocationFailed);
        }
        Err(RegistryError::SourceIdAllocationFailed)
    }

    fn persist(&self) -> Result<(), RegistryError> {
        let layout = RegistryStorageLayout::new(&self.storage_directory);

        let mut body = String::new();
        body.push_str(&format!("version|{}\n", CURRENT_SCHEMA_VERSION));
        body.push_str(&format!("evidence|{}\n", CURRENT_EVIDENCE_SCHEMA_VERSION));
        body.push_str(&format!("namespace|{}\n", encode_field(&self.namespace)));
        body.push_str(&format!("revision|{}\n", self.revision));
        body.push_str(&format!("next_sequence|{}\n", self.next_source_sequence));
        body.push_str(&format!("entries|{}\n", self.entries.len()));

        for entry in self.entries.values() {
            body.push_str("source|");
            body.push_str(match &entry.state {
                SourceState::Live => "live",
                SourceState::Absent => "absent",
                SourceState::Retired(_) => "retired",
            });
            body.push('|');
            body.push_str(&encode_field(entry.id.as_str()));
            body.push('|');
            body.push_str(&encode_field(&entry.continuity.backend_key));
            body.push('|');
            body.push_str(&encode_field(&entry.continuity.continuity_token));
            body.push('|');
            body.push_str(&encode_field(&entry.display_name));
            body.push('|');
            let reason = match &entry.state {
                SourceState::Retired(reason) => reason_to_token(reason),
                _ => "",
            };
            body.push_str(reason);
            body.push('\n');
        }

        let mut candidate = File::create(&layout.candidate)?;
        candidate.write_all(body.as_bytes())?;
        candidate.sync_all()?;
        drop(candidate);

        if layout.backup.exists() {
            fs::remove_file(&layout.backup)?;
        }
        fs::copy(&layout.candidate, &layout.backup)?;
        if layout.current.exists() {
            fs::remove_file(&layout.current)?;
        }
        fs::rename(&layout.candidate, &layout.current)?;
        File::options()
            .read(true)
            .write(true)
            .open(&layout.current)?
            .sync_all()?;
        if let Some(parent) = layout.current.parent() {
            if let Ok(parent_dir) = File::open(parent) {
                parent_dir.sync_all().ok();
            }
        }

        Ok(())
    }
}

impl From<RetiredReason> for PersistedRetiredReason {
    fn from(reason: RetiredReason) -> Self {
        match reason {
            RetiredReason::NativeKeyReuse => Self::NativeKeyReuse,
            RetiredReason::AmbiguousReturn => Self::AmbiguousReturn,
            RetiredReason::MigrationCorruption => Self::MigrationCorruption,
            RetiredReason::NamespaceReset => Self::NamespaceReset,
        }
    }
}

impl From<PersistedRetiredReason> for RetiredReason {
    fn from(reason: PersistedRetiredReason) -> Self {
        match reason {
            PersistedRetiredReason::NativeKeyReuse => Self::NativeKeyReuse,
            PersistedRetiredReason::AmbiguousReturn => Self::AmbiguousReturn,
            PersistedRetiredReason::MigrationCorruption => Self::MigrationCorruption,
            PersistedRetiredReason::NamespaceReset => Self::NamespaceReset,
        }
    }
}

impl PersistedRegistry {
    fn into_current_schema(mut self) -> Result<Self, RegistryError> {
        if self.schema_version == 0 || self.evidence_schema_version == 0 {
            return Err(RegistryError::MigrationFailed);
        }
        if self.schema_version > CURRENT_SCHEMA_VERSION {
            return Err(RegistryError::UnsupportedSchema {
                expected: CURRENT_SCHEMA_VERSION,
                observed: self.schema_version,
            });
        }
        if self.evidence_schema_version > CURRENT_EVIDENCE_SCHEMA_VERSION {
            return Err(RegistryError::UnsupportedSchema {
                expected: CURRENT_EVIDENCE_SCHEMA_VERSION,
                observed: self.evidence_schema_version,
            });
        }

        self.schema_version = CURRENT_SCHEMA_VERSION;
        self.evidence_schema_version = CURRENT_EVIDENCE_SCHEMA_VERSION;
        Ok(self)
    }
}

struct RegistryStorageLayout {
    current: PathBuf,
    backup: PathBuf,
    candidate: PathBuf,
}

impl RegistryStorageLayout {
    fn new(storage_directory: &Path) -> Self {
        let current = storage_directory.join(REGISTRY_FILENAME);
        Self {
            current: current.clone(),
            backup: current.with_extension(REGISTRY_BACKUP_SUFFIX),
            candidate: current.with_extension(REGISTRY_CANDIDATE_SUFFIX),
        }
    }
}
fn generate_namespace() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("ns-{}-{nanos}", std::process::id())
}

fn parse_sequence_from_id(id: &str) -> Option<u64> {
    id.rsplit_once('-')?.1.parse().ok()
}

fn reason_to_token(reason: &RetiredReason) -> &'static str {
    match reason {
        RetiredReason::NativeKeyReuse => "native_key_reuse",
        RetiredReason::AmbiguousReturn => "ambiguous_return",
        RetiredReason::MigrationCorruption => "migration_corruption",
        RetiredReason::NamespaceReset => "namespace_reset",
    }
}

fn token_to_reason(token: &str) -> Option<RetiredReason> {
    match token {
        "native_key_reuse" => Some(RetiredReason::NativeKeyReuse),
        "ambiguous_return" => Some(RetiredReason::AmbiguousReturn),
        "migration_corruption" => Some(RetiredReason::MigrationCorruption),
        "namespace_reset" => Some(RetiredReason::NamespaceReset),
        _ => None,
    }
}

fn encode_field(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte >= 0x20 && byte <= 0x7E && byte != b'|' && byte != b'%' {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push_str(&format!("{:02X}", byte));
        }
    }
    out
}

fn decode_field(value: &str) -> Result<String, RegistryError> {
    let mut bytes = Vec::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            let mut encoded = [0u8; 4];
            let encoded = ch.encode_utf8(&mut encoded);
            bytes.extend(encoded.as_bytes());
            continue;
        }
        let hi = chars.next().ok_or(RegistryError::CorruptRegistry)?;
        let lo = chars.next().ok_or(RegistryError::CorruptRegistry)?;
        let byte = u8::from_str_radix(&format!("{hi}{lo}"), 16)
            .map_err(|_| RegistryError::CorruptRegistry)?;
        bytes.push(byte);
    }
    String::from_utf8(bytes).map_err(|_| RegistryError::CorruptRegistry)
}

fn load_registry(path: &Path) -> Result<PersistedRegistry, RegistryError> {
    if !path.exists() {
        return Err(RegistryError::CorruptRegistry);
    }

    let mut file = File::open(path)?;
    let mut body = String::new();
    file.read_to_string(&mut body)?;

    let mut schema_version = 0;
    let mut evidence_schema_version = 0;
    let mut namespace = String::new();
    let mut revision = 0;
    let mut next_source_sequence = 0;
    let mut expected_entry_count = None;
    let mut entries = Vec::new();

    let mut lines = body.lines().filter(|line| !line.trim().is_empty());
    let mut seen = BTreeSet::new();

    while let Some(line) = lines.next() {
        let Some((key, value)) = line.split_once('|') else {
            return Err(RegistryError::CorruptRegistry);
        };

        if !seen.insert(key.to_string()) {
            return Err(RegistryError::CorruptRegistry);
        }

        match key {
            "version" => {
                schema_version = value.parse().map_err(|_| RegistryError::CorruptRegistry)?;
            }
            "evidence" => {
                evidence_schema_version =
                    value.parse().map_err(|_| RegistryError::CorruptRegistry)?;
            }
            "namespace" => {
                namespace = decode_field(value)?;
            }
            "revision" => {
                revision = value.parse().map_err(|_| RegistryError::CorruptRegistry)?;
            }
            "next_sequence" => {
                next_source_sequence = value.parse().map_err(|_| RegistryError::CorruptRegistry)?;
            }
            "entries" => {
                expected_entry_count = Some(
                    value
                        .parse::<usize>()
                        .map_err(|_| RegistryError::CorruptRegistry)?,
                );
                break;
            }
            _ => return Err(RegistryError::CorruptRegistry),
        }
    }

    let expected = expected_entry_count.ok_or(RegistryError::CorruptRegistry)?;

    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() != 7 {
            return Err(RegistryError::CorruptRegistry);
        }
        if parts[0] != "source" {
            return Err(RegistryError::CorruptRegistry);
        }

        let state = match parts[1] {
            "live" => PersistedSourceState::Live,
            "absent" => PersistedSourceState::Absent,
            "retired" => PersistedSourceState::Retired,
            _ => return Err(RegistryError::CorruptRegistry),
        };

        let retired_reason = if state == PersistedSourceState::Retired {
            if parts[6].is_empty() {
                None
            } else {
                token_to_reason(&decode_field(parts[6])?).map(PersistedRetiredReason::from)
            }
        } else {
            None
        };

        entries.push(PersistedSourceRecord {
            source_id: decode_field(parts[2])?,
            continuity: ContinuityEvidence {
                backend_key: decode_field(parts[3])?,
                continuity_token: decode_field(parts[4])?,
            },
            display_name: decode_field(parts[5])?,
            state,
            retired_reason,
        });
    }

    if entries.len() != expected {
        return Err(RegistryError::CorruptRegistry);
    }

    if namespace.is_empty() {
        return Err(RegistryError::CorruptRegistry);
    }

    Ok(PersistedRegistry {
        schema_version,
        evidence_schema_version,
        namespace,
        revision,
        next_source_sequence,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;

    #[derive(Clone)]
    struct DeterministicAllocator;

    impl SourceIdAllocator for DeterministicAllocator {
        fn next_source_id(&mut self, namespace: &str, sequence: u64) -> String {
            format!("det-{namespace}-{sequence}")
        }
    }

    fn temporary_directory() -> PathBuf {
        let mut path = env::var_os("TEMP")
            .or_else(|| env::var_os("TMP"))
            .map(PathBuf::from)
            .or_else(|| env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        path.push("resonance-agent");
        path.push("identity-tests");
        fs::create_dir_all(&path).expect("failed to create test temp root");
        let pid = std::process::id();
        let thread_id = format!("{:?}", std::thread::current().id());
        let mut attempt = 0u64;
        loop {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let candidate = path.join(format!(
                "resonance-agent-identity-{pid}-{thread_id}-{nanos}-{attempt}"
            ));
            if !candidate.exists() {
                fs::create_dir_all(&candidate)
                    .expect("failed to create deterministic test directory");
                return candidate;
            }
            attempt = attempt.saturating_add(1);
        }
    }

    fn stable(backend_key: &str, continuity_token: &str) -> ObservedContinuity {
        ObservedContinuity::Stable {
            backend_key: backend_key.to_string(),
            continuity_token: continuity_token.to_string(),
        }
    }

    fn evidence(backend_key: &str, continuity_token: &str) -> ContinuityEvidence {
        ContinuityEvidence {
            backend_key: backend_key.to_string(),
            continuity_token: continuity_token.to_string(),
        }
    }

    #[test]
    fn duplicate_names_map_to_distinct_ids() {
        let path = temporary_directory();
        let mut registry = IdentityRegistry::new(&path).unwrap();

        registry
            .apply_observations(&[
                SourceObservation {
                    display_name: "Headset".to_string(),
                    continuity: stable("k", "a"),
                },
                SourceObservation {
                    display_name: "Headset".to_string(),
                    continuity: stable("k", "b"),
                },
            ])
            .unwrap();

        let first = registry
            .source_id_for_continuity(&evidence("k", "a"))
            .unwrap();
        let second = registry
            .source_id_for_continuity(&evidence("k", "b"))
            .unwrap();

        assert_ne!(first, second);
    }

    #[test]
    fn metadata_rename_preserves_source_id() {
        let path = temporary_directory();
        let mut registry = IdentityRegistry::new(&path).unwrap();

        registry
            .apply_observations(&[SourceObservation {
                display_name: "Headset".to_string(),
                continuity: stable("k", "a"),
            }])
            .unwrap();
        let before = registry
            .source_id_for_continuity(&evidence("k", "a"))
            .unwrap();

        registry
            .apply_observations(&[SourceObservation {
                display_name: "Bluetooth Headset".to_string(),
                continuity: stable("k", "a"),
            }])
            .unwrap();
        let after = registry
            .source_id_for_continuity(&evidence("k", "a"))
            .unwrap();

        assert_eq!(before, after);
    }

    #[test]
    fn temporary_disappearance_becomes_absent_and_return_restores_id() {
        let path = temporary_directory();
        let mut registry = IdentityRegistry::new(&path).unwrap();

        registry
            .apply_observations(&[SourceObservation {
                display_name: "Desktop".to_string(),
                continuity: stable("k", "a"),
            }])
            .unwrap();

        let source_id = registry
            .source_id_for_continuity(&evidence("k", "a"))
            .unwrap();
        let stale = registry.apply_observations(&[]).unwrap();
        assert!(matches!(
            registry.resolve(&source_id, &stale),
            Err(RegistryError::SourceUnavailable)
        ));

        let back = registry
            .apply_observations(&[SourceObservation {
                display_name: "Desktop".to_string(),
                continuity: stable("k", "a"),
            }])
            .unwrap();
        assert!(matches!(
            registry.resolve(&source_id, &back),
            Ok(SourceResolution::Live(_))
        ));
    }

    #[test]
    fn ambiguous_return_does_not_keep_prior_id() {
        let path = temporary_directory();
        let mut registry = IdentityRegistry::new(&path).unwrap();

        registry
            .apply_observations(&[SourceObservation {
                display_name: "Headset".to_string(),
                continuity: stable("k", "a"),
            }])
            .unwrap();
        let old = registry
            .source_id_for_continuity(&evidence("k", "a"))
            .unwrap();

        let snapshot = registry
            .apply_observations(&[SourceObservation {
                display_name: "Headset".to_string(),
                continuity: ObservedContinuity::Ambiguous(vec![
                    evidence("k", "a"),
                    evidence("k", "b"),
                ]),
            }])
            .unwrap();

        assert!(matches!(
            registry.resolve(&old, &snapshot),
            Err(RegistryError::SourceRetired)
        ));
    }

    #[test]
    fn native_key_reuse_retires_prior_and_allocates_new() {
        let path = temporary_directory();
        let mut registry = IdentityRegistry::new(&path).unwrap();

        registry
            .apply_observations(&[SourceObservation {
                display_name: "Headset".to_string(),
                continuity: stable("k", "old"),
            }])
            .unwrap();
        let old = registry
            .source_id_for_continuity(&evidence("k", "old"))
            .unwrap();

        let snapshot = registry
            .apply_observations(&[SourceObservation {
                display_name: "Headset".to_string(),
                continuity: stable("k", "new"),
            }])
            .unwrap();
        let replacement = registry
            .source_id_for_continuity(&evidence("k", "new"))
            .unwrap();

        assert_ne!(old, replacement);
        assert!(matches!(
            registry.resolve(&old, &snapshot),
            Err(RegistryError::SourceRetired)
        ));
    }

    #[test]
    fn stale_snapshot_is_rejected_before_resolution() {
        let path = temporary_directory();
        let mut registry = IdentityRegistry::new(&path).unwrap();

        let snapshot = registry
            .apply_observations(&[SourceObservation {
                display_name: "Desktop".to_string(),
                continuity: stable("k", "a"),
            }])
            .unwrap();

        let _ = registry
            .apply_observations(&[SourceObservation {
                display_name: "Desktop".to_string(),
                continuity: stable("k", "b"),
            }])
            .unwrap();

        let source = SourceId::new("id-never-present").unwrap();
        assert!(matches!(
            registry.resolve(&source, &snapshot),
            Err(RegistryError::SnapshotStale { .. })
        ));
    }

    #[test]
    fn namespace_reset_invalidates_existing_ids() {
        let path = temporary_directory();
        let mut registry = IdentityRegistry::new(&path).unwrap();

        registry
            .apply_observations(&[SourceObservation {
                display_name: "Desktop".to_string(),
                continuity: stable("k", "a"),
            }])
            .unwrap();
        let old = registry
            .source_id_for_continuity(&evidence("k", "a"))
            .unwrap();
        let previous_namespace = registry.namespace().to_string();

        registry.reset_namespace().unwrap();
        assert_ne!(previous_namespace, registry.namespace());
        assert!(matches!(
            registry.resolve(&old, &registry.snapshot()),
            Err(RegistryError::UnknownSource)
        ));
    }

    #[test]
    fn no_substitution_guarantees_exact_id_only() {
        let path = temporary_directory();
        let mut registry = IdentityRegistry::new(&path).unwrap();

        registry
            .apply_observations(&[
                SourceObservation {
                    display_name: "One".to_string(),
                    continuity: stable("k", "a"),
                },
                SourceObservation {
                    display_name: "Two".to_string(),
                    continuity: stable("k", "b"),
                },
            ])
            .unwrap();

        let first = registry
            .source_id_for_continuity(&evidence("k", "a"))
            .unwrap();
        let snapshot = registry
            .apply_observations(&[SourceObservation {
                display_name: "Two Renamed".to_string(),
                continuity: stable("k", "c"),
            }])
            .unwrap();

        assert!(matches!(
            registry.resolve(&first, &snapshot),
            Err(RegistryError::UnknownSource) | Err(RegistryError::SourceUnavailable)
        ));
    }

    #[test]
    fn restart_reloads_namespace_and_mappings() {
        let path = temporary_directory();
        let snapshot;
        {
            let mut registry = IdentityRegistry::new(&path).unwrap();
            registry
                .apply_observations(&[SourceObservation {
                    display_name: "Desktop".to_string(),
                    continuity: stable("k", "a"),
                }])
                .unwrap();
            snapshot = registry.snapshot();
        }

        let reloaded = IdentityRegistry::new(&path).unwrap();
        assert_eq!(snapshot.namespace, reloaded.namespace());
        assert!(reloaded
            .source_id_for_continuity(&evidence("k", "a"))
            .is_some());
    }

    #[test]
    fn corrupted_current_falls_back_to_backup() {
        let path = temporary_directory();
        let layout = RegistryStorageLayout::new(&path);

        {
            let mut registry = IdentityRegistry::new(&path).unwrap();
            registry
                .apply_observations(&[SourceObservation {
                    display_name: "Desktop".to_string(),
                    continuity: stable("k", "a"),
                }])
                .unwrap();
        }

        let mut backup = String::new();
        File::open(&layout.backup)
            .and_then(|mut file| file.read_to_string(&mut backup))
            .unwrap();
        fs::write(&layout.current, "bad payload").unwrap();

        let registry = IdentityRegistry::new(&path).unwrap();
        assert!(registry
            .source_id_for_continuity(&evidence("k", "a"))
            .is_some());

        let mut maybe_same_namespace = String::new();
        if backup.contains("namespace") {
            maybe_same_namespace.push_str(&backup);
        }
        drop(maybe_same_namespace);
    }

    #[test]
    fn deterministic_allocator_generates_predictable_ids() {
        let path = temporary_directory();
        let mut registry = IdentityRegistry::with_allocator(&path, DeterministicAllocator).unwrap();

        registry
            .apply_observations(&[
                SourceObservation {
                    display_name: "A".to_string(),
                    continuity: stable("k", "a"),
                },
                SourceObservation {
                    display_name: "B".to_string(),
                    continuity: stable("k", "b"),
                },
            ])
            .unwrap();

        let first = registry
            .source_id_for_continuity(&evidence("k", "a"))
            .unwrap();
        let second = registry
            .source_id_for_continuity(&evidence("k", "b"))
            .unwrap();

        assert_eq!(first.as_str(), format!("det-{}-1", registry.namespace()));
        assert_eq!(second.as_str(), format!("det-{}-2", registry.namespace()));
    }
}
