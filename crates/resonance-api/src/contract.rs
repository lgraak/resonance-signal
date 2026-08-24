//! Transport-independent provider contract definitions.

use std::error::Error;
use std::fmt;

pub use resonance_core::signal::{
    AudioFrame, ChannelCount, ChannelLayout, ChannelLevel, ChannelPosition, FrameTimestamp,
    LevelFrame, Sample, SampleRate, SignalDataError, SignalFormatError, SignalWindow,
    SpectrumFrame, SpectrumWindow,
};

/// The semantic version of the transport-independent audio contract.
pub const AUDIO_CONTRACT_VERSION: ContractVersion = ContractVersion::new(0, 1);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ContractVersion {
    major: u16,
    minor: u16,
}

impl ContractVersion {
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    pub const fn major(self) -> u16 {
        self.major
    }

    pub const fn minor(self) -> u16 {
        self.minor
    }
}

/// An opaque provider-assigned source identifier.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SourceId(String);

impl SourceId {
    pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(IdentifierError::Empty);
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An opaque identifier for one uninterrupted source stream.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct StreamId(String);

impl StreamId {
    pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(IdentifierError::Empty);
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentifierError {
    Empty,
}

impl fmt::Display for IdentifierError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("identifier must contain a non-whitespace character")
    }
}

impl Error for IdentifierError {}

/// A source category for presentation and policy decisions, not device lookup.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SourceKind {
    Playback,
    Microphone,
    Virtual,
    Other,
}

/// A role for which a platform default can be resolved at subscription time.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DefaultSource {
    Playback,
    Capture,
}

/// A transport-independent request for a source.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum SourceSelector {
    Default(DefaultSource),
    Id(SourceId),
}

/// A data product a consumer may request independently.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SignalProduct {
    /// Interleaved normalized waveform samples.
    Waveform,
    /// Per-channel RMS and sample-peak levels.
    Levels,
    /// Per-channel single-sided linear magnitude spectra.
    Spectrum,
}

/// A request for one or more products from one or more sources.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubscriptionRequest {
    sources: Vec<SourceSelector>,
    products: Vec<SignalProduct>,
}

impl SubscriptionRequest {
    pub fn new(
        sources: impl Into<Vec<SourceSelector>>,
        products: impl Into<Vec<SignalProduct>>,
    ) -> Result<Self, SubscriptionRequestError> {
        let sources = sources.into();
        let products = products.into();

        if sources.is_empty() {
            return Err(SubscriptionRequestError::NoSources);
        }
        if products.is_empty() {
            return Err(SubscriptionRequestError::NoProducts);
        }
        if let Some(duplicate) = first_duplicate(&sources) {
            return Err(SubscriptionRequestError::DuplicateSource(duplicate.clone()));
        }
        if let Some(duplicate) = first_duplicate(&products) {
            return Err(SubscriptionRequestError::DuplicateProduct(*duplicate));
        }

        Ok(Self { sources, products })
    }

    pub fn sources(&self) -> &[SourceSelector] {
        &self.sources
    }

    pub fn products(&self) -> &[SignalProduct] {
        &self.products
    }
}

fn first_duplicate<T: PartialEq>(values: &[T]) -> Option<&T> {
    values
        .iter()
        .enumerate()
        .find_map(|(index, value)| values[..index].contains(value).then_some(value))
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubscriptionRequestError {
    NoSources,
    NoProducts,
    DuplicateSource(SourceSelector),
    DuplicateProduct(SignalProduct),
}

impl fmt::Display for SubscriptionRequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSources => formatter.write_str("subscription must request at least one source"),
            Self::NoProducts => {
                formatter.write_str("subscription must request at least one signal product")
            }
            Self::DuplicateSource(source) => write!(formatter, "duplicate source {source:?}"),
            Self::DuplicateProduct(product) => write!(formatter, "duplicate product {product:?}"),
        }
    }
}

impl Error for SubscriptionRequestError {}

/// The negotiated, fixed format of one uninterrupted stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamDescriptor {
    stream_id: StreamId,
    source_id: SourceId,
    source_kind: SourceKind,
    sample_rate: SampleRate,
    channels: ChannelLayout,
}

impl StreamDescriptor {
    pub fn new(
        stream_id: StreamId,
        source_id: SourceId,
        source_kind: SourceKind,
        sample_rate: SampleRate,
        channels: ChannelLayout,
    ) -> Self {
        Self {
            stream_id,
            source_id,
            source_kind,
            sample_rate,
            channels,
        }
    }

    pub fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    pub fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    pub const fn source_kind(&self) -> SourceKind {
        self.source_kind
    }

    pub const fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    pub fn channels(&self) -> &ChannelLayout {
        &self.channels
    }
}

/// One data payload associated with an uninterrupted stream.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum SignalPayload {
    Waveform(AudioFrame),
    Levels(LevelFrame),
    Spectrum(SpectrumFrame),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SignalPacket {
    stream_id: StreamId,
    payload: SignalPayload,
}

impl SignalPacket {
    pub fn new(stream_id: StreamId, payload: SignalPayload) -> Self {
        Self { stream_id, payload }
    }

    pub fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    pub fn payload(&self) -> &SignalPayload {
        &self.payload
    }
}

/// Stable, platform-neutral failure categories.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ErrorKind {
    SourceUnavailable,
    PermissionDenied,
    StreamInterrupted,
    UnsupportedFormat,
    InvalidRequest,
    ResourceExhausted,
    Internal,
}

/// The part of a request or stream affected by an error.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ErrorScope {
    Subscription,
    Source(SourceId),
    Stream(StreamId),
}

/// Machine-actionable recovery guidance.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RetryHint {
    RetryNow,
    RetryLater,
    WaitForSource,
    RequestPermission,
    ChangeFormat,
    DoNotRetry,
}

/// A provider failure with stable categories and non-stable diagnostic text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderError {
    kind: ErrorKind,
    scope: ErrorScope,
    retry_hint: RetryHint,
    message: String,
}

impl ProviderError {
    pub fn new(
        kind: ErrorKind,
        scope: ErrorScope,
        retry_hint: RetryHint,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            scope,
            retry_hint,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    pub fn scope(&self) -> &ErrorScope {
        &self.scope
    }

    pub const fn retry_hint(&self) -> RetryHint {
        self.retry_hint
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ProviderError {}

/// Why an uninterrupted stream ended.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StreamEndReason {
    ConsumerCancelled,
    SourceEnded,
    SourceReconfigured,
    ProviderShutdown,
    Failed,
}

/// Lifecycle and data events emitted by a provider subscription.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum StreamEvent {
    Started(StreamDescriptor),
    Data(SignalPacket),
    Error(ProviderError),
    Ended {
        stream_id: StreamId,
        reason: StreamEndReason,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscription_supports_multiple_sources_and_products() {
        let request = SubscriptionRequest::new(
            vec![
                SourceSelector::Default(DefaultSource::Playback),
                SourceSelector::Id(SourceId::new("microphone:desk").unwrap()),
            ],
            vec![SignalProduct::Waveform, SignalProduct::Levels],
        )
        .unwrap();

        assert_eq!(request.sources().len(), 2);
        assert_eq!(request.products().len(), 2);
    }

    #[test]
    fn subscription_rejects_empty_and_duplicate_members() {
        assert_eq!(
            SubscriptionRequest::new(Vec::<SourceSelector>::new(), vec![SignalProduct::Waveform]),
            Err(SubscriptionRequestError::NoSources)
        );
        assert_eq!(
            SubscriptionRequest::new(
                vec![SourceSelector::Default(DefaultSource::Playback)],
                vec![SignalProduct::Waveform, SignalProduct::Waveform],
            ),
            Err(SubscriptionRequestError::DuplicateProduct(
                SignalProduct::Waveform
            ))
        );
    }

    #[test]
    fn source_and_stream_identifiers_are_opaque_but_non_empty() {
        assert_eq!(SourceId::new("  "), Err(IdentifierError::Empty));
        assert_eq!(StreamId::new("stream-1").unwrap().as_str(), "stream-1");
    }

    #[test]
    fn errors_keep_machine_action_separate_from_diagnostics() {
        let error = ProviderError::new(
            ErrorKind::PermissionDenied,
            ErrorScope::Subscription,
            RetryHint::RequestPermission,
            "capture permission was denied",
        );

        assert_eq!(error.kind(), ErrorKind::PermissionDenied);
        assert_eq!(error.retry_hint(), RetryHint::RequestPermission);
        assert_eq!(error.to_string(), "capture permission was denied");
    }
}
