//! Validated, immutable recovery policy configuration.
//!
//! Configuration is agent-internal data. Validation has no access to capture
//! owners, hardware, clocks, timers, threads, or supervisor mutation.

#![allow(dead_code)]

use std::num::NonZeroU64;
use std::sync::Arc;
use std::time::Duration;

const CONFIGURATION_SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct RecoveryConfigurationVersion(NonZeroU64);

impl RecoveryConfigurationVersion {
    pub(crate) const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    pub(crate) const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Stable identity for one validated configuration definition.
///
/// The explicit version supports operator-controlled revisioning. The content
/// fingerprint prevents changed values from reusing the same version without
/// invalidating already evaluated recovery assumptions.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct RecoveryConfigurationIdentity {
    schema_version: u16,
    version: RecoveryConfigurationVersion,
    fingerprint: u128,
}

impl RecoveryConfigurationIdentity {
    pub(crate) const fn version(self) -> RecoveryConfigurationVersion {
        self.version
    }

    pub(crate) const fn fingerprint(self) -> u128 {
        self.fingerprint
    }

    #[cfg(test)]
    pub(crate) const fn test_only(version: u64, fingerprint: u128) -> Self {
        let Some(version) = RecoveryConfigurationVersion::new(version) else {
            panic!("test configuration version must be nonzero");
        };
        Self {
            schema_version: CONFIGURATION_SCHEMA_VERSION,
            version,
            fingerprint,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExhaustionBehavior {
    RemainStoppedUntilNewIntent,
    RemainStoppedUntilConfigurationChange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AutomaticRecoveryAttemptBudget {
    pub(crate) maximum_automatic_recovery_attempts: u32,
    pub(crate) exhaustion_behavior: ExhaustionBehavior,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DelayResetBehavior {
    NewIntent,
    NewRecoveryEpisodeAfterStableRun,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CooldownConfiguration {
    Disabled,
    Required {
        minimum_delay: Duration,
        reset: DelayResetBehavior,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BackoffDelayStrategy {
    Fixed {
        delay: Duration,
    },
    Linear {
        initial_delay: Duration,
        increment: Duration,
    },
    Exponential {
        initial_delay: Duration,
        multiplier: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JitterRequirement {
    Forbidden,
    Required { maximum: Duration },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BackoffConfiguration {
    Disabled,
    Required {
        strategy: BackoffDelayStrategy,
        maximum_delay: Duration,
        jitter: JitterRequirement,
        reset: DelayResetBehavior,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AdditionalEvidenceRequirement {
    SourceAvailable,
    SupportedFormatAvailable,
    ResourcePressureCleared,
    ChangedPrecondition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FailureDisposition {
    Retryable,
    NonRetryable,
    AdditionalEvidenceRequired(AdditionalEvidenceRequirement),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum RecoveryFailureClass {
    OwnerConstructionFailure = 0,
    DeviceUnavailable = 1,
    SourceReconfigured = 2,
    Interrupted = 3,
    ResourceExhausted = 4,
    UnsupportedFormat = 5,
    InternalFailure = 6,
    StartupFailure = 7,
    WorkerPanic = 8,
}

const FAILURE_CLASS_COUNT: usize = 9;
const FAILURE_CLASSES: [RecoveryFailureClass; FAILURE_CLASS_COUNT] = [
    RecoveryFailureClass::OwnerConstructionFailure,
    RecoveryFailureClass::DeviceUnavailable,
    RecoveryFailureClass::SourceReconfigured,
    RecoveryFailureClass::Interrupted,
    RecoveryFailureClass::ResourceExhausted,
    RecoveryFailureClass::UnsupportedFormat,
    RecoveryFailureClass::InternalFailure,
    RecoveryFailureClass::StartupFailure,
    RecoveryFailureClass::WorkerPanic,
];

/// Classification of stable agent-level causes, never platform error codes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FailureClassificationPolicy {
    dispositions: [FailureDisposition; FAILURE_CLASS_COUNT],
}

impl FailureClassificationPolicy {
    pub(crate) const fn uniform(disposition: FailureDisposition) -> Self {
        Self {
            dispositions: [disposition; FAILURE_CLASS_COUNT],
        }
    }

    pub(crate) const fn with_classification(
        mut self,
        class: RecoveryFailureClass,
        disposition: FailureDisposition,
    ) -> Self {
        self.dispositions[class as usize] = disposition;
        self
    }

    pub(crate) const fn classification(self, class: RecoveryFailureClass) -> FailureDisposition {
        self.dispositions[class as usize]
    }

    fn permits_any_automatic_recovery(self) -> bool {
        self.dispositions
            .iter()
            .any(|disposition| *disposition != FailureDisposition::NonRetryable)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StableRunEvidence {
    SupervisorRunningState,
    ContinuousFrameDelivery,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StableRunResetPolicy {
    NewIntentOnly,
    StableRunRequired {
        evidence: StableRunEvidence,
        minimum_duration: Duration,
    },
}

/// Untrusted definition at the validation boundary.
///
/// Optional fields deliberately represent missing external input even though
/// file, environment, and reload integrations are deferred.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RecoveryConfigurationInput {
    pub(crate) version: Option<RecoveryConfigurationVersion>,
    pub(crate) attempt_budget: Option<AutomaticRecoveryAttemptBudget>,
    pub(crate) cooldown: Option<CooldownConfiguration>,
    pub(crate) backoff: Option<BackoffConfiguration>,
    pub(crate) failure_classification: Option<FailureClassificationPolicy>,
    pub(crate) stable_run_reset: Option<StableRunResetPolicy>,
}

impl RecoveryConfigurationInput {
    /// Explicit fail-closed definition used while recovery execution is absent.
    pub(crate) const fn recovery_disabled(version: RecoveryConfigurationVersion) -> Self {
        Self {
            version: Some(version),
            attempt_budget: Some(AutomaticRecoveryAttemptBudget {
                maximum_automatic_recovery_attempts: 0,
                exhaustion_behavior: ExhaustionBehavior::RemainStoppedUntilNewIntent,
            }),
            cooldown: Some(CooldownConfiguration::Disabled),
            backoff: Some(BackoffConfiguration::Disabled),
            failure_classification: Some(FailureClassificationPolicy::uniform(
                FailureDisposition::NonRetryable,
            )),
            stable_run_reset: Some(StableRunResetPolicy::NewIntentOnly),
        }
    }

    pub(crate) fn validate(
        self,
    ) -> Result<RecoveryConfigurationSnapshot, RecoveryConfigurationError> {
        let configuration = RecoveryConfiguration {
            version: self
                .version
                .ok_or(RecoveryConfigurationError::MissingRequiredField(
                    RecoveryConfigurationField::Version,
                ))?,
            attempt_budget: self.attempt_budget.ok_or(
                RecoveryConfigurationError::MissingRequiredField(
                    RecoveryConfigurationField::AttemptBudget,
                ),
            )?,
            cooldown: self
                .cooldown
                .ok_or(RecoveryConfigurationError::MissingRequiredField(
                    RecoveryConfigurationField::Cooldown,
                ))?,
            backoff: self
                .backoff
                .ok_or(RecoveryConfigurationError::MissingRequiredField(
                    RecoveryConfigurationField::Backoff,
                ))?,
            failure_classification: self.failure_classification.ok_or(
                RecoveryConfigurationError::MissingRequiredField(
                    RecoveryConfigurationField::FailureClassification,
                ),
            )?,
            stable_run_reset: self.stable_run_reset.ok_or(
                RecoveryConfigurationError::MissingRequiredField(
                    RecoveryConfigurationField::StableRunReset,
                ),
            )?,
        };
        configuration.validate()?;
        let identity = configuration.identity();
        Ok(RecoveryConfigurationSnapshot {
            identity,
            configuration: Arc::new(configuration),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RecoveryConfiguration {
    version: RecoveryConfigurationVersion,
    attempt_budget: AutomaticRecoveryAttemptBudget,
    cooldown: CooldownConfiguration,
    backoff: BackoffConfiguration,
    failure_classification: FailureClassificationPolicy,
    stable_run_reset: StableRunResetPolicy,
}

impl RecoveryConfiguration {
    fn validate(&self) -> Result<(), RecoveryConfigurationError> {
        let recovery_enabled = self.attempt_budget.maximum_automatic_recovery_attempts > 0;
        let classified_for_recovery = self.failure_classification.permits_any_automatic_recovery();

        if recovery_enabled != classified_for_recovery {
            return Err(RecoveryConfigurationError::BudgetClassificationConflict);
        }
        validate_failure_classification(self.failure_classification)?;

        if !recovery_enabled {
            if self.cooldown != CooldownConfiguration::Disabled
                || self.backoff != BackoffConfiguration::Disabled
                || self.stable_run_reset != StableRunResetPolicy::NewIntentOnly
            {
                return Err(RecoveryConfigurationError::DisabledRecoveryHasActivePolicy);
            }
            return Ok(());
        }

        let (minimum_delay, cooldown_reset) = match self.cooldown {
            CooldownConfiguration::Required {
                minimum_delay,
                reset,
            } if !minimum_delay.is_zero() => (minimum_delay, reset),
            CooldownConfiguration::Required { .. } => {
                return Err(RecoveryConfigurationError::ZeroDuration(
                    RecoveryDurationField::CooldownMinimum,
                ));
            }
            CooldownConfiguration::Disabled => {
                return Err(RecoveryConfigurationError::RecoveryRequiresCooldown);
            }
        };

        let (maximum_delay, backoff_reset) = match self.backoff {
            BackoffConfiguration::Required {
                strategy,
                maximum_delay,
                jitter,
                reset,
            } => {
                validate_backoff(strategy, maximum_delay, jitter)?;
                (maximum_delay, reset)
            }
            BackoffConfiguration::Disabled => {
                return Err(RecoveryConfigurationError::RecoveryRequiresBackoff);
            }
        };

        if minimum_delay > maximum_delay {
            return Err(RecoveryConfigurationError::CooldownExceedsMaximumDelay);
        }
        if cooldown_reset != backoff_reset {
            return Err(RecoveryConfigurationError::DelayResetConflict);
        }

        match (self.stable_run_reset, cooldown_reset) {
            (StableRunResetPolicy::NewIntentOnly, DelayResetBehavior::NewIntent) => {}
            (
                StableRunResetPolicy::StableRunRequired {
                    minimum_duration, ..
                },
                DelayResetBehavior::NewRecoveryEpisodeAfterStableRun,
            ) if !minimum_duration.is_zero() => {}
            (StableRunResetPolicy::StableRunRequired { .. }, _) => {
                return Err(RecoveryConfigurationError::StableRunResetConflict);
            }
            (StableRunResetPolicy::NewIntentOnly, _) => {
                return Err(RecoveryConfigurationError::StableRunResetConflict);
            }
        }

        Ok(())
    }

    fn identity(&self) -> RecoveryConfigurationIdentity {
        let mut fingerprint = StableFingerprint::new();
        fingerprint.write_u16(CONFIGURATION_SCHEMA_VERSION);
        encode_budget(&mut fingerprint, self.attempt_budget);
        encode_cooldown(&mut fingerprint, self.cooldown);
        encode_backoff(&mut fingerprint, self.backoff);
        for class in FAILURE_CLASSES {
            fingerprint.write_u8(class as u8);
            encode_disposition(
                &mut fingerprint,
                self.failure_classification.classification(class),
            );
        }
        encode_stable_run_reset(&mut fingerprint, self.stable_run_reset);
        RecoveryConfigurationIdentity {
            schema_version: CONFIGURATION_SCHEMA_VERSION,
            version: self.version,
            fingerprint: fingerprint.finish(),
        }
    }
}

/// Owned immutable configuration suitable for recovery evaluation snapshots.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RecoveryConfigurationSnapshot {
    identity: RecoveryConfigurationIdentity,
    configuration: Arc<RecoveryConfiguration>,
}

impl RecoveryConfigurationSnapshot {
    pub(crate) const fn identity(&self) -> RecoveryConfigurationIdentity {
        self.identity
    }

    pub(crate) fn is_same_configuration(&self, other: &Self) -> bool {
        self.identity == other.identity && self.configuration == other.configuration
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecoveryConfigurationField {
    Version,
    AttemptBudget,
    Cooldown,
    Backoff,
    FailureClassification,
    StableRunReset,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecoveryDurationField {
    CooldownMinimum,
    BackoffInitial,
    BackoffIncrement,
    BackoffMaximum,
    JitterMaximum,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecoveryConfigurationError {
    MissingRequiredField(RecoveryConfigurationField),
    BudgetClassificationConflict,
    DisabledRecoveryHasActivePolicy,
    RecoveryRequiresCooldown,
    RecoveryRequiresBackoff,
    ZeroDuration(RecoveryDurationField),
    BackoffInitialExceedsMaximum,
    BackoffIncrementExceedsMaximum,
    ExponentialMultiplierTooSmall,
    JitterExceedsMaximumDelay,
    CooldownExceedsMaximumDelay,
    DelayResetConflict,
    StableRunResetConflict,
    FailureEvidenceConflict(RecoveryFailureClass),
}

fn validate_failure_classification(
    policy: FailureClassificationPolicy,
) -> Result<(), RecoveryConfigurationError> {
    let guarded_classes = [
        (
            RecoveryFailureClass::DeviceUnavailable,
            &[
                AdditionalEvidenceRequirement::SourceAvailable,
                AdditionalEvidenceRequirement::ChangedPrecondition,
            ][..],
        ),
        (
            RecoveryFailureClass::SourceReconfigured,
            &[
                AdditionalEvidenceRequirement::SupportedFormatAvailable,
                AdditionalEvidenceRequirement::ChangedPrecondition,
            ][..],
        ),
        (
            RecoveryFailureClass::ResourceExhausted,
            &[
                AdditionalEvidenceRequirement::ResourcePressureCleared,
                AdditionalEvidenceRequirement::ChangedPrecondition,
            ][..],
        ),
        (
            RecoveryFailureClass::UnsupportedFormat,
            &[
                AdditionalEvidenceRequirement::SupportedFormatAvailable,
                AdditionalEvidenceRequirement::ChangedPrecondition,
            ][..],
        ),
    ];

    for (class, allowed_evidence) in guarded_classes {
        match policy.classification(class) {
            FailureDisposition::NonRetryable => {}
            FailureDisposition::AdditionalEvidenceRequired(requirement)
                if allowed_evidence.contains(&requirement) => {}
            _ => return Err(RecoveryConfigurationError::FailureEvidenceConflict(class)),
        }
    }
    Ok(())
}

fn validate_backoff(
    strategy: BackoffDelayStrategy,
    maximum_delay: Duration,
    jitter: JitterRequirement,
) -> Result<(), RecoveryConfigurationError> {
    if maximum_delay.is_zero() {
        return Err(RecoveryConfigurationError::ZeroDuration(
            RecoveryDurationField::BackoffMaximum,
        ));
    }

    match strategy {
        BackoffDelayStrategy::Fixed { delay } => {
            validate_nonzero(delay, RecoveryDurationField::BackoffInitial)?;
            if delay > maximum_delay {
                return Err(RecoveryConfigurationError::BackoffInitialExceedsMaximum);
            }
        }
        BackoffDelayStrategy::Linear {
            initial_delay,
            increment,
        } => {
            validate_nonzero(initial_delay, RecoveryDurationField::BackoffInitial)?;
            validate_nonzero(increment, RecoveryDurationField::BackoffIncrement)?;
            if initial_delay > maximum_delay {
                return Err(RecoveryConfigurationError::BackoffInitialExceedsMaximum);
            }
            if increment > maximum_delay {
                return Err(RecoveryConfigurationError::BackoffIncrementExceedsMaximum);
            }
        }
        BackoffDelayStrategy::Exponential {
            initial_delay,
            multiplier,
        } => {
            validate_nonzero(initial_delay, RecoveryDurationField::BackoffInitial)?;
            if initial_delay > maximum_delay {
                return Err(RecoveryConfigurationError::BackoffInitialExceedsMaximum);
            }
            if multiplier < 2 {
                return Err(RecoveryConfigurationError::ExponentialMultiplierTooSmall);
            }
        }
    }

    if let JitterRequirement::Required { maximum } = jitter {
        validate_nonzero(maximum, RecoveryDurationField::JitterMaximum)?;
        if maximum > maximum_delay {
            return Err(RecoveryConfigurationError::JitterExceedsMaximumDelay);
        }
    }
    Ok(())
}

fn validate_nonzero(
    duration: Duration,
    field: RecoveryDurationField,
) -> Result<(), RecoveryConfigurationError> {
    if duration.is_zero() {
        return Err(RecoveryConfigurationError::ZeroDuration(field));
    }
    Ok(())
}

fn encode_budget(fingerprint: &mut StableFingerprint, budget: AutomaticRecoveryAttemptBudget) {
    fingerprint.write_u32(budget.maximum_automatic_recovery_attempts);
    fingerprint.write_u8(match budget.exhaustion_behavior {
        ExhaustionBehavior::RemainStoppedUntilNewIntent => 0,
        ExhaustionBehavior::RemainStoppedUntilConfigurationChange => 1,
    });
}

fn encode_cooldown(fingerprint: &mut StableFingerprint, cooldown: CooldownConfiguration) {
    match cooldown {
        CooldownConfiguration::Disabled => fingerprint.write_u8(0),
        CooldownConfiguration::Required {
            minimum_delay,
            reset,
        } => {
            fingerprint.write_u8(1);
            fingerprint.write_duration(minimum_delay);
            fingerprint.write_u8(encode_delay_reset(reset));
        }
    }
}

fn encode_backoff(fingerprint: &mut StableFingerprint, backoff: BackoffConfiguration) {
    match backoff {
        BackoffConfiguration::Disabled => fingerprint.write_u8(0),
        BackoffConfiguration::Required {
            strategy,
            maximum_delay,
            jitter,
            reset,
        } => {
            fingerprint.write_u8(1);
            match strategy {
                BackoffDelayStrategy::Fixed { delay } => {
                    fingerprint.write_u8(0);
                    fingerprint.write_duration(delay);
                }
                BackoffDelayStrategy::Linear {
                    initial_delay,
                    increment,
                } => {
                    fingerprint.write_u8(1);
                    fingerprint.write_duration(initial_delay);
                    fingerprint.write_duration(increment);
                }
                BackoffDelayStrategy::Exponential {
                    initial_delay,
                    multiplier,
                } => {
                    fingerprint.write_u8(2);
                    fingerprint.write_duration(initial_delay);
                    fingerprint.write_u32(multiplier);
                }
            }
            fingerprint.write_duration(maximum_delay);
            match jitter {
                JitterRequirement::Forbidden => fingerprint.write_u8(0),
                JitterRequirement::Required { maximum } => {
                    fingerprint.write_u8(1);
                    fingerprint.write_duration(maximum);
                }
            }
            fingerprint.write_u8(encode_delay_reset(reset));
        }
    }
}

fn encode_disposition(fingerprint: &mut StableFingerprint, disposition: FailureDisposition) {
    match disposition {
        FailureDisposition::Retryable => fingerprint.write_u8(0),
        FailureDisposition::NonRetryable => fingerprint.write_u8(1),
        FailureDisposition::AdditionalEvidenceRequired(requirement) => {
            fingerprint.write_u8(2);
            fingerprint.write_u8(match requirement {
                AdditionalEvidenceRequirement::SourceAvailable => 0,
                AdditionalEvidenceRequirement::SupportedFormatAvailable => 1,
                AdditionalEvidenceRequirement::ResourcePressureCleared => 2,
                AdditionalEvidenceRequirement::ChangedPrecondition => 3,
            });
        }
    }
}

fn encode_stable_run_reset(fingerprint: &mut StableFingerprint, policy: StableRunResetPolicy) {
    match policy {
        StableRunResetPolicy::NewIntentOnly => fingerprint.write_u8(0),
        StableRunResetPolicy::StableRunRequired {
            evidence,
            minimum_duration,
        } => {
            fingerprint.write_u8(1);
            fingerprint.write_u8(match evidence {
                StableRunEvidence::SupervisorRunningState => 0,
                StableRunEvidence::ContinuousFrameDelivery => 1,
            });
            fingerprint.write_duration(minimum_duration);
        }
    }
}

const fn encode_delay_reset(reset: DelayResetBehavior) -> u8 {
    match reset {
        DelayResetBehavior::NewIntent => 0,
        DelayResetBehavior::NewRecoveryEpisodeAfterStableRun => 1,
    }
}

/// Fixed, documented byte encoding plus two independent FNV-style lanes.
///
/// This is an identity fingerprint, not a security or authentication primitive.
struct StableFingerprint {
    first: u64,
    second: u64,
}

impl StableFingerprint {
    const fn new() -> Self {
        Self {
            first: 0xcbf2_9ce4_8422_2325,
            second: 0x8422_2325_cbf2_9ce4,
        }
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.first ^= u64::from(*byte);
            self.first = self.first.wrapping_mul(0x0000_0100_0000_01b3);
            self.second ^= u64::from(*byte ^ 0xa5);
            self.second = self.second.wrapping_mul(0x0000_0100_0000_01e7);
        }
    }

    fn write_u8(&mut self, value: u8) {
        self.write(&[value]);
    }

    fn write_u16(&mut self, value: u16) {
        self.write(&value.to_le_bytes());
    }

    fn write_u32(&mut self, value: u32) {
        self.write(&value.to_le_bytes());
    }

    fn write_duration(&mut self, value: Duration) {
        self.write(&value.as_secs().to_le_bytes());
        self.write(&value.subsec_nanos().to_le_bytes());
    }

    const fn finish(self) -> u128 {
        ((self.first as u128) << 64) | self.second as u128
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retry_state::RetryState;

    const TEST_VERSION: RecoveryConfigurationVersion =
        RecoveryConfigurationVersion::new(7).expect("test version is nonzero");

    fn enabled_test_input() -> RecoveryConfigurationInput {
        RecoveryConfigurationInput {
            version: Some(TEST_VERSION),
            attempt_budget: Some(AutomaticRecoveryAttemptBudget {
                maximum_automatic_recovery_attempts: 3,
                exhaustion_behavior: ExhaustionBehavior::RemainStoppedUntilNewIntent,
            }),
            cooldown: Some(CooldownConfiguration::Required {
                minimum_delay: Duration::from_secs(1),
                reset: DelayResetBehavior::NewRecoveryEpisodeAfterStableRun,
            }),
            backoff: Some(BackoffConfiguration::Required {
                strategy: BackoffDelayStrategy::Exponential {
                    initial_delay: Duration::from_secs(1),
                    multiplier: 2,
                },
                maximum_delay: Duration::from_secs(30),
                jitter: JitterRequirement::Required {
                    maximum: Duration::from_millis(250),
                },
                reset: DelayResetBehavior::NewRecoveryEpisodeAfterStableRun,
            }),
            failure_classification: Some(
                FailureClassificationPolicy::uniform(FailureDisposition::NonRetryable)
                    .with_classification(
                        RecoveryFailureClass::DeviceUnavailable,
                        FailureDisposition::AdditionalEvidenceRequired(
                            AdditionalEvidenceRequirement::SourceAvailable,
                        ),
                    )
                    .with_classification(
                        RecoveryFailureClass::Interrupted,
                        FailureDisposition::Retryable,
                    ),
            ),
            stable_run_reset: Some(StableRunResetPolicy::StableRunRequired {
                evidence: StableRunEvidence::ContinuousFrameDelivery,
                minimum_duration: Duration::from_secs(60),
            }),
        }
    }

    #[test]
    fn valid_equivalent_definitions_have_stable_identity() {
        let first = enabled_test_input().validate().expect("valid test input");
        let second = enabled_test_input().validate().expect("valid test input");

        assert_eq!(first.identity(), second.identity());
        assert!(first.is_same_configuration(&second));
        assert_eq!(first.identity().version(), TEST_VERSION);
    }

    #[test]
    fn content_or_version_changes_invalidate_prior_identity() {
        let original = enabled_test_input().validate().expect("valid test input");

        let mut changed_content = enabled_test_input();
        changed_content.attempt_budget = Some(AutomaticRecoveryAttemptBudget {
            maximum_automatic_recovery_attempts: 4,
            exhaustion_behavior: ExhaustionBehavior::RemainStoppedUntilNewIntent,
        });
        let changed_content = changed_content.validate().expect("valid changed input");

        let mut changed_version = enabled_test_input();
        changed_version.version = RecoveryConfigurationVersion::new(8);
        let changed_version = changed_version.validate().expect("valid changed input");

        assert_ne!(original.identity(), changed_content.identity());
        assert_ne!(original.identity(), changed_version.identity());
        assert!(!original.is_same_configuration(&changed_content));
    }

    #[test]
    fn all_required_fields_fail_closed_when_missing() {
        let fields = [
            RecoveryConfigurationField::Version,
            RecoveryConfigurationField::AttemptBudget,
            RecoveryConfigurationField::Cooldown,
            RecoveryConfigurationField::Backoff,
            RecoveryConfigurationField::FailureClassification,
            RecoveryConfigurationField::StableRunReset,
        ];

        for field in fields {
            let mut input = enabled_test_input();
            match field {
                RecoveryConfigurationField::Version => input.version = None,
                RecoveryConfigurationField::AttemptBudget => input.attempt_budget = None,
                RecoveryConfigurationField::Cooldown => input.cooldown = None,
                RecoveryConfigurationField::Backoff => input.backoff = None,
                RecoveryConfigurationField::FailureClassification => {
                    input.failure_classification = None;
                }
                RecoveryConfigurationField::StableRunReset => input.stable_run_reset = None,
            }
            assert_eq!(
                input.validate(),
                Err(RecoveryConfigurationError::MissingRequiredField(field))
            );
        }
    }

    #[test]
    fn rejects_budget_classification_conflict_and_disabled_active_policy() {
        let mut no_budget = enabled_test_input();
        no_budget.attempt_budget = Some(AutomaticRecoveryAttemptBudget {
            maximum_automatic_recovery_attempts: 0,
            exhaustion_behavior: ExhaustionBehavior::RemainStoppedUntilNewIntent,
        });
        assert_eq!(
            no_budget.validate(),
            Err(RecoveryConfigurationError::BudgetClassificationConflict)
        );

        let mut disabled_with_cooldown =
            RecoveryConfigurationInput::recovery_disabled(TEST_VERSION);
        disabled_with_cooldown.cooldown = Some(CooldownConfiguration::Required {
            minimum_delay: Duration::from_secs(1),
            reset: DelayResetBehavior::NewIntent,
        });
        assert_eq!(
            disabled_with_cooldown.validate(),
            Err(RecoveryConfigurationError::DisabledRecoveryHasActivePolicy)
        );
    }

    #[test]
    fn rejects_zero_or_impossible_delays_and_reset_conflicts() {
        let mut zero_cooldown = enabled_test_input();
        zero_cooldown.cooldown = Some(CooldownConfiguration::Required {
            minimum_delay: Duration::ZERO,
            reset: DelayResetBehavior::NewRecoveryEpisodeAfterStableRun,
        });
        assert_eq!(
            zero_cooldown.validate(),
            Err(RecoveryConfigurationError::ZeroDuration(
                RecoveryDurationField::CooldownMinimum
            ))
        );

        let mut excessive_initial = enabled_test_input();
        excessive_initial.backoff = Some(BackoffConfiguration::Required {
            strategy: BackoffDelayStrategy::Fixed {
                delay: Duration::from_secs(31),
            },
            maximum_delay: Duration::from_secs(30),
            jitter: JitterRequirement::Forbidden,
            reset: DelayResetBehavior::NewRecoveryEpisodeAfterStableRun,
        });
        assert_eq!(
            excessive_initial.validate(),
            Err(RecoveryConfigurationError::BackoffInitialExceedsMaximum)
        );

        let mut conflicting_reset = enabled_test_input();
        conflicting_reset.stable_run_reset = Some(StableRunResetPolicy::NewIntentOnly);
        assert_eq!(
            conflicting_reset.validate(),
            Err(RecoveryConfigurationError::StableRunResetConflict)
        );
    }

    #[test]
    fn guarded_failure_classes_require_matching_typed_evidence() {
        let mut unsafe_resource_retry = enabled_test_input();
        unsafe_resource_retry.failure_classification = Some(
            unsafe_resource_retry
                .failure_classification
                .expect("test policy is present")
                .with_classification(
                    RecoveryFailureClass::ResourceExhausted,
                    FailureDisposition::Retryable,
                ),
        );

        assert_eq!(
            unsafe_resource_retry.validate(),
            Err(RecoveryConfigurationError::FailureEvidenceConflict(
                RecoveryFailureClass::ResourceExhausted
            ))
        );
    }

    #[test]
    fn accepted_snapshot_is_owned_and_immutable() {
        let mut input = enabled_test_input();
        let accepted = input.clone().validate().expect("valid test input");
        let captured_identity = accepted.identity();

        input.version = RecoveryConfigurationVersion::new(99);
        let changed = input.validate().expect("changed input remains valid");

        assert_eq!(accepted.identity(), captured_identity);
        assert_eq!(accepted.identity().version(), TEST_VERSION);
        assert_ne!(accepted.identity(), changed.identity());
    }

    #[test]
    fn validation_is_data_only_and_does_not_mutate_retry_state() {
        let disabled = RecoveryConfigurationInput::recovery_disabled(TEST_VERSION)
            .validate()
            .expect("disabled configuration is valid");
        let mut state = RetryState::<4>::new().expect("history capacity is nonzero");
        let generation = state
            .explicit_start(disabled.identity())
            .expect("new intent accepts configuration identity");
        let before = state.snapshot().expect("active intent has a snapshot");

        let _accepted = enabled_test_input().validate().expect("valid test input");

        let after = state.snapshot().expect("active intent has a snapshot");
        assert_eq!(before, after);
        assert_eq!(state.intent_generation(), Some(generation));
    }
}
