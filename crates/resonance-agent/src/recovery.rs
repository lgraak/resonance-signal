//! Side-effect-free recovery policy evaluation.
//!
//! This module represents policy authorization only. It does not create or
//! control capture owners, wait for time or hardware, or mutate retry state.

// CaptureSupervisor evaluates this policy and records the result, but recovery
// execution remains separately gated and disabled.
#![allow(dead_code)]

use resonance_api::contract::RetryHint;

use crate::recovery_config::{
    AdditionalEvidenceRequirement, BackoffConfiguration, CooldownConfiguration, FailureDisposition,
    RecoveryConfigurationIdentity, RecoveryConfigurationSnapshot, RecoveryFailureClass,
};
use crate::retry_state::{CooldownState, RetryFailureCause, RetrySnapshot};

/// The capture-intent state against which a recovery decision is evaluated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RecoveryContext {
    pub(crate) desired_running: bool,
    pub(crate) current_intent_generation: u64,
    pub(crate) evaluated_intent_generation: u64,
}

/// Stable, agent-internal classifications for one completed capture attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecoveryCause {
    ExplicitStop,
    NormalShutdown,
    DeviceUnavailable(DeviceUnavailableCause),
    SourceReconfigured(SourceReconfigurationCause),
    Interrupted,
    ResourceExhausted,
    UnsupportedFormat,
    InternalFailure,
    StartupFailure,
    WorkerPanic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DeviceUnavailableCause {
    Removed,
    Invalidated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SourceReconfigurationCause {
    DefaultEndpointChanged,
    FormatChanged,
}

/// Source-policy evidence relevant to guarded recovery causes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecoverySourcePolicy {
    Pinned,
    FollowDefault,
}

/// A structured snapshot of lifecycle and future policy-owned state.
///
/// Optional fields make evidence gaps representable. The evaluator fails
/// closed when a field required by the applicable policy row is absent.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RecoveryEvidence {
    pub(crate) stream_started: Option<bool>,
    pub(crate) terminal_event_delivered: Option<bool>,
    pub(crate) owner_completed: Option<bool>,
    pub(crate) resources_released: Option<bool>,
    pub(crate) retry_hint: Option<RetryHint>,
    pub(crate) attempts_remaining: Option<bool>,
    pub(crate) cooldown_complete: Option<bool>,
    pub(crate) source_available: Option<bool>,
    pub(crate) source_policy: Option<RecoverySourcePolicy>,
    pub(crate) replacement_source_resolved: Option<bool>,
    pub(crate) supported_format_available: Option<bool>,
    pub(crate) pressure_cleared: Option<bool>,
    pub(crate) changed_precondition: Option<bool>,
}

/// The authorization returned by recovery policy evaluation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecoveryDecision {
    RemainStopped(RecoveryDecisionReason),
    Wait(RecoveryDecisionReason),
    PermitReplacement(RecoveryDecisionReason),
}

/// Stable explanations for policy decisions and tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecoveryDecisionReason {
    ExplicitStop,
    StaleIntent,
    StaleConfiguration,
    CleanupPending,
    TerminalBoundaryPending,
    MissingEvidence,
    InconsistentEvidence,
    RetryVetoed,
    ConfiguredNonRetryable,
    NormalShutdown,
    SourceUnavailable,
    SourceAvailable,
    PinnedSource,
    ReplacementSourcePending,
    ReplacementSourceReady,
    SupportedFormatUnavailable,
    SupportedFormatAvailable,
    RetryBudgetExhausted,
    CooldownPending,
    RetryEligible,
    PressureClearancePending,
    PressureCleared,
    ChangedPreconditionPending,
    UnsupportedFormat,
    InternalFailure,
    StartupFailure,
    WorkerPanic,
}

/// Evaluates validated configuration, immutable retry state, and typed
/// lifecycle evidence without mutating state or performing recovery.
pub(crate) fn evaluate_recovery_policy(
    configuration: &RecoveryConfigurationSnapshot,
    current_configuration_id: RecoveryConfigurationIdentity,
    retry_state: &RetrySnapshot,
    context: RecoveryContext,
    cause: RecoveryCause,
    mut evidence: RecoveryEvidence,
) -> RecoveryDecision {
    if !context.desired_running || cause == RecoveryCause::ExplicitStop {
        return RecoveryDecision::RemainStopped(RecoveryDecisionReason::ExplicitStop);
    }

    if context.evaluated_intent_generation != context.current_intent_generation {
        return RecoveryDecision::RemainStopped(RecoveryDecisionReason::StaleIntent);
    }

    if configuration.identity() != current_configuration_id
        || retry_state.configuration_id != configuration.identity()
    {
        return RecoveryDecision::RemainStopped(RecoveryDecisionReason::StaleConfiguration);
    }

    if retry_state.desired_running != context.desired_running
        || retry_state.intent_generation.get() != context.evaluated_intent_generation
    {
        return RecoveryDecision::RemainStopped(RecoveryDecisionReason::InconsistentEvidence);
    }

    if let Some(decision) = evaluate_common_preconditions(context, cause, evidence) {
        return decision;
    }

    let Some(failure) = retry_state.last_failure else {
        return evaluate_recovery(context, cause, evidence);
    };
    if !cause_matches_failure(cause, failure.cause) {
        return RecoveryDecision::RemainStopped(RecoveryDecisionReason::InconsistentEvidence);
    }

    let class = failure_class(failure.cause);
    let disposition = configuration.failure_disposition(class);
    match disposition {
        FailureDisposition::NonRetryable => {
            return RecoveryDecision::RemainStopped(RecoveryDecisionReason::ConfiguredNonRetryable);
        }
        FailureDisposition::Retryable | FailureDisposition::AdditionalEvidenceRequired(_) => {}
    }

    if let Some(decision) = retry_veto(evidence.retry_hint) {
        return decision;
    }

    let automatic_attempts_started = retry_state
        .recovery_episode
        .map_or(0, |episode| episode.automatic_recovery_attempts_started);
    let attempts_remaining = retry_state
        .recovery_episode
        .is_none_or(|episode| episode.exhaustion.is_none())
        && automatic_attempts_started
            < u64::from(configuration.maximum_automatic_recovery_attempts());
    evidence.attempts_remaining = Some(attempts_remaining);
    if !attempts_remaining {
        return RecoveryDecision::RemainStopped(RecoveryDecisionReason::RetryBudgetExhausted);
    }

    let delay_required = matches!(
        configuration.cooldown(),
        CooldownConfiguration::Required { .. }
    ) || matches!(
        configuration.backoff(),
        BackoffConfiguration::Required { .. }
    );
    evidence.cooldown_complete =
        Some(!delay_required || matches!(retry_state.cooldown, CooldownState::Satisfied { .. }));
    if evidence.cooldown_complete == Some(false) {
        return RecoveryDecision::Wait(RecoveryDecisionReason::CooldownPending);
    }

    if let FailureDisposition::AdditionalEvidenceRequired(requirement) = disposition {
        if let Some(decision) = evaluate_additional_evidence(requirement, evidence) {
            return decision;
        }
    }

    evaluate_recovery(context, cause, evidence)
}

fn failure_class(cause: RetryFailureCause) -> RecoveryFailureClass {
    match cause {
        RetryFailureCause::OwnerConstructionFailure => {
            RecoveryFailureClass::OwnerConstructionFailure
        }
        RetryFailureCause::DeviceUnavailable(_) => RecoveryFailureClass::DeviceUnavailable,
        RetryFailureCause::SourceReconfigured(_) => RecoveryFailureClass::SourceReconfigured,
        RetryFailureCause::Interrupted => RecoveryFailureClass::Interrupted,
        RetryFailureCause::ResourceExhausted => RecoveryFailureClass::ResourceExhausted,
        RetryFailureCause::UnsupportedFormat => RecoveryFailureClass::UnsupportedFormat,
        RetryFailureCause::InternalFailure => RecoveryFailureClass::InternalFailure,
        RetryFailureCause::StartupFailure => RecoveryFailureClass::StartupFailure,
        RetryFailureCause::WorkerPanic => RecoveryFailureClass::WorkerPanic,
    }
}

fn cause_matches_failure(cause: RecoveryCause, failure: RetryFailureCause) -> bool {
    match (cause, failure) {
        (RecoveryCause::StartupFailure, RetryFailureCause::OwnerConstructionFailure)
        | (RecoveryCause::StartupFailure, RetryFailureCause::StartupFailure)
        | (RecoveryCause::Interrupted, RetryFailureCause::Interrupted)
        | (RecoveryCause::ResourceExhausted, RetryFailureCause::ResourceExhausted)
        | (RecoveryCause::UnsupportedFormat, RetryFailureCause::UnsupportedFormat)
        | (RecoveryCause::InternalFailure, RetryFailureCause::InternalFailure)
        | (RecoveryCause::WorkerPanic, RetryFailureCause::WorkerPanic) => true,
        (
            RecoveryCause::DeviceUnavailable(expected),
            RetryFailureCause::DeviceUnavailable(actual),
        ) => expected == actual,
        (
            RecoveryCause::SourceReconfigured(expected),
            RetryFailureCause::SourceReconfigured(actual),
        ) => expected == actual,
        _ => false,
    }
}

fn evaluate_additional_evidence(
    requirement: AdditionalEvidenceRequirement,
    evidence: RecoveryEvidence,
) -> Option<RecoveryDecision> {
    let (value, pending_reason) = match requirement {
        AdditionalEvidenceRequirement::SourceAvailable => (
            evidence.source_available,
            RecoveryDecisionReason::SourceUnavailable,
        ),
        AdditionalEvidenceRequirement::SupportedFormatAvailable => (
            evidence.supported_format_available,
            RecoveryDecisionReason::SupportedFormatUnavailable,
        ),
        AdditionalEvidenceRequirement::ResourcePressureCleared => (
            evidence.pressure_cleared,
            RecoveryDecisionReason::PressureClearancePending,
        ),
        AdditionalEvidenceRequirement::ChangedPrecondition => (
            evidence.changed_precondition,
            RecoveryDecisionReason::ChangedPreconditionPending,
        ),
    };
    match value {
        Some(true) => None,
        Some(false) => Some(RecoveryDecision::Wait(pending_reason)),
        None => Some(missing_evidence()),
    }
}

/// Evaluates recovery authorization without performing recovery behavior.
pub(crate) fn evaluate_recovery(
    context: RecoveryContext,
    cause: RecoveryCause,
    evidence: RecoveryEvidence,
) -> RecoveryDecision {
    if let Some(decision) = evaluate_common_preconditions(context, cause, evidence) {
        return decision;
    }

    match cause {
        RecoveryCause::ExplicitStop => {
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::ExplicitStop)
        }
        RecoveryCause::NormalShutdown => {
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::NormalShutdown)
        }
        RecoveryCause::DeviceUnavailable(_) => evaluate_device_unavailable(evidence),
        RecoveryCause::SourceReconfigured(SourceReconfigurationCause::DefaultEndpointChanged) => {
            evaluate_default_endpoint_change(evidence)
        }
        RecoveryCause::SourceReconfigured(SourceReconfigurationCause::FormatChanged) => {
            evaluate_format_change(evidence)
        }
        RecoveryCause::Interrupted => evaluate_interruption(evidence),
        RecoveryCause::ResourceExhausted => evaluate_resource_exhaustion(evidence),
        RecoveryCause::UnsupportedFormat => evaluate_stopped_cause(
            evidence.retry_hint,
            RecoveryDecisionReason::UnsupportedFormat,
        ),
        RecoveryCause::InternalFailure => {
            evaluate_stopped_cause(evidence.retry_hint, RecoveryDecisionReason::InternalFailure)
        }
        RecoveryCause::StartupFailure => {
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::StartupFailure)
        }
        RecoveryCause::WorkerPanic => {
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::WorkerPanic)
        }
    }
}

fn evaluate_common_preconditions(
    context: RecoveryContext,
    cause: RecoveryCause,
    evidence: RecoveryEvidence,
) -> Option<RecoveryDecision> {
    if !context.desired_running || cause == RecoveryCause::ExplicitStop {
        return Some(RecoveryDecision::RemainStopped(
            RecoveryDecisionReason::ExplicitStop,
        ));
    }

    if context.evaluated_intent_generation != context.current_intent_generation {
        return Some(RecoveryDecision::RemainStopped(
            RecoveryDecisionReason::StaleIntent,
        ));
    }

    let Some(stream_started) = evidence.stream_started else {
        return Some(missing_evidence());
    };
    let Some(owner_completed) = evidence.owner_completed else {
        return Some(missing_evidence());
    };
    let Some(resources_released) = evidence.resources_released else {
        return Some(missing_evidence());
    };

    if !owner_completed || !resources_released {
        return Some(RecoveryDecision::Wait(
            RecoveryDecisionReason::CleanupPending,
        ));
    }

    if stream_started {
        let Some(terminal_event_delivered) = evidence.terminal_event_delivered else {
            return Some(missing_evidence());
        };
        if !terminal_event_delivered {
            return Some(RecoveryDecision::Wait(
                RecoveryDecisionReason::TerminalBoundaryPending,
            ));
        }
    } else if evidence.terminal_event_delivered == Some(true) {
        return Some(RecoveryDecision::RemainStopped(
            RecoveryDecisionReason::InconsistentEvidence,
        ));
    }

    None
}

fn evaluate_device_unavailable(evidence: RecoveryEvidence) -> RecoveryDecision {
    if let Some(decision) = retry_veto(evidence.retry_hint) {
        return decision;
    }
    if evidence.retry_hint != Some(RetryHint::WaitForSource) {
        return evidence.retry_hint.map_or_else(missing_evidence, |_| {
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::InconsistentEvidence)
        });
    }
    if evidence.source_policy.is_none()
        || evidence.source_available.is_none()
        || evidence.attempts_remaining.is_none()
    {
        return missing_evidence();
    }
    if evidence.attempts_remaining == Some(false) {
        return RecoveryDecision::RemainStopped(RecoveryDecisionReason::RetryBudgetExhausted);
    }

    match evidence.source_available {
        Some(true) => RecoveryDecision::PermitReplacement(RecoveryDecisionReason::SourceAvailable),
        Some(false) => RecoveryDecision::Wait(RecoveryDecisionReason::SourceUnavailable),
        None => missing_evidence(),
    }
}

fn evaluate_default_endpoint_change(evidence: RecoveryEvidence) -> RecoveryDecision {
    if let Some(decision) = retry_veto(evidence.retry_hint) {
        return decision;
    }
    if evidence.retry_hint.is_none()
        || evidence.source_policy.is_none()
        || evidence.replacement_source_resolved.is_none()
        || evidence.attempts_remaining.is_none()
    {
        return missing_evidence();
    }
    if evidence.source_policy == Some(RecoverySourcePolicy::Pinned) {
        return RecoveryDecision::RemainStopped(RecoveryDecisionReason::PinnedSource);
    }
    if evidence.attempts_remaining == Some(false) {
        return RecoveryDecision::RemainStopped(RecoveryDecisionReason::RetryBudgetExhausted);
    }

    match evidence.replacement_source_resolved {
        Some(true) => {
            RecoveryDecision::PermitReplacement(RecoveryDecisionReason::ReplacementSourceReady)
        }
        Some(false) => RecoveryDecision::Wait(RecoveryDecisionReason::ReplacementSourcePending),
        None => missing_evidence(),
    }
}

fn evaluate_format_change(evidence: RecoveryEvidence) -> RecoveryDecision {
    if let Some(decision) = retry_veto(evidence.retry_hint) {
        return decision;
    }
    if evidence.retry_hint.is_none()
        || evidence.supported_format_available.is_none()
        || evidence.attempts_remaining.is_none()
    {
        return missing_evidence();
    }
    if evidence.attempts_remaining == Some(false) {
        return RecoveryDecision::RemainStopped(RecoveryDecisionReason::RetryBudgetExhausted);
    }

    match evidence.supported_format_available {
        Some(true) => {
            RecoveryDecision::PermitReplacement(RecoveryDecisionReason::SupportedFormatAvailable)
        }
        Some(false) => RecoveryDecision::Wait(RecoveryDecisionReason::SupportedFormatUnavailable),
        None => missing_evidence(),
    }
}

fn evaluate_interruption(evidence: RecoveryEvidence) -> RecoveryDecision {
    if let Some(decision) = retry_veto(evidence.retry_hint) {
        return decision;
    }
    let Some(attempts_remaining) = evidence.attempts_remaining else {
        return missing_evidence();
    };
    if !attempts_remaining {
        return RecoveryDecision::RemainStopped(RecoveryDecisionReason::RetryBudgetExhausted);
    }

    match evidence.retry_hint {
        Some(RetryHint::RetryNow) => {
            RecoveryDecision::PermitReplacement(RecoveryDecisionReason::RetryEligible)
        }
        Some(RetryHint::RetryLater) => match evidence.cooldown_complete {
            Some(true) => {
                RecoveryDecision::PermitReplacement(RecoveryDecisionReason::RetryEligible)
            }
            Some(false) => RecoveryDecision::Wait(RecoveryDecisionReason::CooldownPending),
            None => missing_evidence(),
        },
        Some(_) => RecoveryDecision::RemainStopped(RecoveryDecisionReason::InconsistentEvidence),
        None => missing_evidence(),
    }
}

fn evaluate_resource_exhaustion(evidence: RecoveryEvidence) -> RecoveryDecision {
    if let Some(decision) = retry_veto(evidence.retry_hint) {
        return decision;
    }
    if evidence.retry_hint.is_none()
        || evidence.attempts_remaining.is_none()
        || evidence.cooldown_complete.is_none()
        || evidence.pressure_cleared.is_none()
    {
        return missing_evidence();
    }
    if evidence.retry_hint != Some(RetryHint::RetryLater) {
        return RecoveryDecision::RemainStopped(RecoveryDecisionReason::InconsistentEvidence);
    }
    if evidence.attempts_remaining == Some(false) {
        return RecoveryDecision::RemainStopped(RecoveryDecisionReason::RetryBudgetExhausted);
    }
    if evidence.cooldown_complete == Some(false) || evidence.pressure_cleared == Some(false) {
        return RecoveryDecision::Wait(RecoveryDecisionReason::PressureClearancePending);
    }

    RecoveryDecision::PermitReplacement(RecoveryDecisionReason::PressureCleared)
}

fn evaluate_stopped_cause(
    retry_hint: Option<RetryHint>,
    reason: RecoveryDecisionReason,
) -> RecoveryDecision {
    if retry_hint.is_none() {
        return missing_evidence();
    }
    if let Some(decision) = retry_veto(retry_hint) {
        return decision;
    }
    RecoveryDecision::RemainStopped(reason)
}

fn retry_veto(retry_hint: Option<RetryHint>) -> Option<RecoveryDecision> {
    (retry_hint == Some(RetryHint::DoNotRetry)).then_some(RecoveryDecision::RemainStopped(
        RecoveryDecisionReason::RetryVetoed,
    ))
}

const fn missing_evidence() -> RecoveryDecision {
    RecoveryDecision::RemainStopped(RecoveryDecisionReason::MissingEvidence)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use crate::recovery_config::{
        AutomaticRecoveryAttemptBudget, BackoffConfiguration, BackoffDelayStrategy,
        DelayResetBehavior, ExhaustionBehavior, FailureClassificationPolicy, JitterRequirement,
        RecoveryConfigurationInput, RecoveryConfigurationVersion, StableRunResetPolicy,
    };
    use crate::retry_state::{
        CooldownEligibilityMarker, CooldownEvidenceId, ExhaustionReason,
        RecoveryEpisodeResetEvidence, ResetEvidenceId, RetryState,
    };

    fn context() -> RecoveryContext {
        RecoveryContext {
            desired_running: true,
            current_intent_generation: 7,
            evaluated_intent_generation: 7,
        }
    }

    fn completed_stream() -> RecoveryEvidence {
        RecoveryEvidence {
            stream_started: Some(true),
            terminal_event_delivered: Some(true),
            owner_completed: Some(true),
            resources_released: Some(true),
            ..RecoveryEvidence::default()
        }
    }

    fn completed_startup_attempt() -> RecoveryEvidence {
        RecoveryEvidence {
            stream_started: Some(false),
            terminal_event_delivered: Some(false),
            owner_completed: Some(true),
            resources_released: Some(true),
            ..RecoveryEvidence::default()
        }
    }

    fn enabled_configuration(version: u64, budget: u32) -> RecoveryConfigurationSnapshot {
        let version = RecoveryConfigurationVersion::new(version).expect("test version is nonzero");
        RecoveryConfigurationInput {
            version: Some(version),
            attempt_budget: Some(AutomaticRecoveryAttemptBudget {
                maximum_automatic_recovery_attempts: budget,
                exhaustion_behavior: ExhaustionBehavior::RemainStoppedUntilNewIntent,
            }),
            cooldown: Some(CooldownConfiguration::Required {
                minimum_delay: Duration::from_secs(1),
                reset: DelayResetBehavior::NewIntent,
            }),
            backoff: Some(BackoffConfiguration::Required {
                strategy: BackoffDelayStrategy::Fixed {
                    delay: Duration::from_secs(1),
                },
                maximum_delay: Duration::from_secs(1),
                jitter: JitterRequirement::Forbidden,
                reset: DelayResetBehavior::NewIntent,
            }),
            failure_classification: Some(
                FailureClassificationPolicy::uniform(FailureDisposition::NonRetryable)
                    .with_classification(
                        RecoveryFailureClass::Interrupted,
                        FailureDisposition::Retryable,
                    )
                    .with_classification(
                        RecoveryFailureClass::DeviceUnavailable,
                        FailureDisposition::AdditionalEvidenceRequired(
                            AdditionalEvidenceRequirement::SourceAvailable,
                        ),
                    )
                    .with_classification(
                        RecoveryFailureClass::UnsupportedFormat,
                        FailureDisposition::AdditionalEvidenceRequired(
                            AdditionalEvidenceRequirement::SupportedFormatAvailable,
                        ),
                    ),
            ),
            stable_run_reset: Some(StableRunResetPolicy::NewIntentOnly),
        }
        .validate()
        .expect("enabled test configuration is valid")
    }

    fn disabled_configuration(version: u64) -> RecoveryConfigurationSnapshot {
        RecoveryConfigurationInput::recovery_disabled(
            RecoveryConfigurationVersion::new(version).expect("test version is nonzero"),
        )
        .validate()
        .expect("disabled test configuration is valid")
    }

    fn failed_state(
        configuration: &RecoveryConfigurationSnapshot,
        failure: RetryFailureCause,
    ) -> RetryState<8> {
        let mut state = RetryState::new().expect("test history capacity is nonzero");
        let generation = state
            .explicit_start(configuration.identity())
            .expect("test intent starts");
        let attempt = state
            .commit_initial_attempt(generation)
            .expect("initial attempt is committed");
        state
            .record_failure(attempt, failure)
            .expect("failure is recorded");
        state
            .record_cleanup_complete(attempt)
            .expect("failed attempt has no remaining resources");
        state
    }

    fn policy_context(snapshot: &RetrySnapshot) -> RecoveryContext {
        RecoveryContext {
            desired_running: snapshot.desired_running,
            current_intent_generation: snapshot.intent_generation.get(),
            evaluated_intent_generation: snapshot.intent_generation.get(),
        }
    }

    fn satisfy_test_cooldown(state: &mut RetryState<8>, marker_value: u64) {
        let snapshot = state.snapshot().expect("failed state has a snapshot");
        let marker = CooldownEligibilityMarker(marker_value);
        state
            .require_cooldown(&snapshot, marker)
            .expect("waiting state accepts a cooldown requirement");
        state
            .satisfy_cooldown(
                snapshot.intent_generation,
                marker,
                CooldownEvidenceId(marker_value),
            )
            .expect("matching cooldown evidence is accepted");
    }

    fn policy_evidence(retry_hint: RetryHint) -> RecoveryEvidence {
        RecoveryEvidence {
            retry_hint: Some(retry_hint),
            attempts_remaining: Some(true),
            cooldown_complete: Some(true),
            ..completed_startup_attempt()
        }
    }

    #[test]
    fn explicit_stop_overrides_otherwise_permitted_recovery() {
        let mut stopped = context();
        stopped.desired_running = false;
        let evidence = RecoveryEvidence {
            retry_hint: Some(RetryHint::RetryNow),
            attempts_remaining: Some(true),
            ..completed_stream()
        };

        assert_eq!(
            evaluate_recovery(stopped, RecoveryCause::Interrupted, evidence),
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::ExplicitStop)
        );
        assert_eq!(
            evaluate_recovery(
                context(),
                RecoveryCause::ExplicitStop,
                RecoveryEvidence::default()
            ),
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::ExplicitStop)
        );
    }

    #[test]
    fn stale_intent_cannot_authorize_recovery() {
        let stale = RecoveryContext {
            evaluated_intent_generation: 6,
            ..context()
        };
        let evidence = RecoveryEvidence {
            retry_hint: Some(RetryHint::RetryNow),
            attempts_remaining: Some(true),
            ..completed_stream()
        };

        assert_eq!(
            evaluate_recovery(stale, RecoveryCause::Interrupted, evidence),
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::StaleIntent)
        );
    }

    #[test]
    fn incomplete_cleanup_and_terminal_delivery_block_replacement() {
        let cleanup_pending = RecoveryEvidence {
            owner_completed: Some(false),
            resources_released: Some(false),
            ..completed_stream()
        };
        assert_eq!(
            evaluate_recovery(context(), RecoveryCause::Interrupted, cleanup_pending),
            RecoveryDecision::Wait(RecoveryDecisionReason::CleanupPending)
        );

        let terminal_pending = RecoveryEvidence {
            terminal_event_delivered: Some(false),
            retry_hint: Some(RetryHint::RetryNow),
            attempts_remaining: Some(true),
            ..completed_stream()
        };
        assert_eq!(
            evaluate_recovery(context(), RecoveryCause::Interrupted, terminal_pending),
            RecoveryDecision::Wait(RecoveryDecisionReason::TerminalBoundaryPending)
        );
    }

    #[test]
    fn missing_or_inconsistent_evidence_fails_closed() {
        assert_eq!(
            evaluate_recovery(
                context(),
                RecoveryCause::Interrupted,
                RecoveryEvidence::default()
            ),
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::MissingEvidence)
        );

        let inconsistent = RecoveryEvidence {
            terminal_event_delivered: Some(true),
            ..completed_startup_attempt()
        };
        assert_eq!(
            evaluate_recovery(context(), RecoveryCause::StartupFailure, inconsistent),
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::InconsistentEvidence)
        );
    }

    #[test]
    fn normal_shutdown_remains_stopped() {
        assert_eq!(
            evaluate_recovery(context(), RecoveryCause::NormalShutdown, completed_stream()),
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::NormalShutdown)
        );
    }

    #[test]
    fn removed_and_invalidated_devices_require_availability_evidence() {
        for cause in [
            DeviceUnavailableCause::Removed,
            DeviceUnavailableCause::Invalidated,
        ] {
            let waiting = RecoveryEvidence {
                retry_hint: Some(RetryHint::WaitForSource),
                attempts_remaining: Some(true),
                source_policy: Some(RecoverySourcePolicy::Pinned),
                source_available: Some(false),
                ..completed_stream()
            };
            assert_eq!(
                evaluate_recovery(context(), RecoveryCause::DeviceUnavailable(cause), waiting),
                RecoveryDecision::Wait(RecoveryDecisionReason::SourceUnavailable)
            );

            let missing_availability = RecoveryEvidence {
                source_available: None,
                ..waiting
            };
            assert_eq!(
                evaluate_recovery(
                    context(),
                    RecoveryCause::DeviceUnavailable(cause),
                    missing_availability
                ),
                RecoveryDecision::RemainStopped(RecoveryDecisionReason::MissingEvidence)
            );

            let available = RecoveryEvidence {
                source_available: Some(true),
                ..waiting
            };
            assert_eq!(
                evaluate_recovery(
                    context(),
                    RecoveryCause::DeviceUnavailable(cause),
                    available
                ),
                RecoveryDecision::PermitReplacement(RecoveryDecisionReason::SourceAvailable)
            );
        }
    }

    #[test]
    fn default_endpoint_change_requires_follow_policy_and_resolved_replacement() {
        let evidence = RecoveryEvidence {
            retry_hint: Some(RetryHint::RetryNow),
            attempts_remaining: Some(true),
            source_policy: Some(RecoverySourcePolicy::Pinned),
            replacement_source_resolved: Some(true),
            ..completed_stream()
        };
        let cause =
            RecoveryCause::SourceReconfigured(SourceReconfigurationCause::DefaultEndpointChanged);
        assert_eq!(
            evaluate_recovery(context(), cause, evidence),
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::PinnedSource)
        );
        assert_eq!(
            evaluate_recovery(
                context(),
                cause,
                RecoveryEvidence {
                    source_policy: Some(RecoverySourcePolicy::FollowDefault),
                    replacement_source_resolved: Some(false),
                    ..evidence
                }
            ),
            RecoveryDecision::Wait(RecoveryDecisionReason::ReplacementSourcePending)
        );
        assert_eq!(
            evaluate_recovery(
                context(),
                cause,
                RecoveryEvidence {
                    source_policy: Some(RecoverySourcePolicy::FollowDefault),
                    ..evidence
                }
            ),
            RecoveryDecision::PermitReplacement(RecoveryDecisionReason::ReplacementSourceReady)
        );
    }

    #[test]
    fn format_change_requires_supported_fresh_negotiation() {
        let cause = RecoveryCause::SourceReconfigured(SourceReconfigurationCause::FormatChanged);
        let evidence = RecoveryEvidence {
            retry_hint: Some(RetryHint::ChangeFormat),
            attempts_remaining: Some(true),
            supported_format_available: Some(false),
            ..completed_stream()
        };
        assert_eq!(
            evaluate_recovery(context(), cause, evidence),
            RecoveryDecision::Wait(RecoveryDecisionReason::SupportedFormatUnavailable)
        );
        assert_eq!(
            evaluate_recovery(
                context(),
                cause,
                RecoveryEvidence {
                    supported_format_available: Some(true),
                    ..evidence
                }
            ),
            RecoveryDecision::PermitReplacement(RecoveryDecisionReason::SupportedFormatAvailable)
        );
    }

    #[test]
    fn interruption_obeys_retry_hint_cooldown_and_budget() {
        let immediate = RecoveryEvidence {
            retry_hint: Some(RetryHint::RetryNow),
            attempts_remaining: Some(true),
            ..completed_stream()
        };
        assert_eq!(
            evaluate_recovery(context(), RecoveryCause::Interrupted, immediate),
            RecoveryDecision::PermitReplacement(RecoveryDecisionReason::RetryEligible)
        );

        let delayed = RecoveryEvidence {
            retry_hint: Some(RetryHint::RetryLater),
            cooldown_complete: Some(false),
            ..immediate
        };
        assert_eq!(
            evaluate_recovery(context(), RecoveryCause::Interrupted, delayed),
            RecoveryDecision::Wait(RecoveryDecisionReason::CooldownPending)
        );
        assert_eq!(
            evaluate_recovery(
                context(),
                RecoveryCause::Interrupted,
                RecoveryEvidence {
                    attempts_remaining: Some(false),
                    ..immediate
                }
            ),
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::RetryBudgetExhausted)
        );
    }

    #[test]
    fn resource_exhaustion_requires_guarded_pressure_evidence() {
        let missing_pressure = RecoveryEvidence {
            retry_hint: Some(RetryHint::RetryLater),
            attempts_remaining: Some(true),
            cooldown_complete: Some(true),
            ..completed_stream()
        };
        assert_eq!(
            evaluate_recovery(
                context(),
                RecoveryCause::ResourceExhausted,
                missing_pressure
            ),
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::MissingEvidence)
        );

        let pressure_present = RecoveryEvidence {
            pressure_cleared: Some(false),
            ..missing_pressure
        };
        assert_eq!(
            evaluate_recovery(
                context(),
                RecoveryCause::ResourceExhausted,
                pressure_present
            ),
            RecoveryDecision::Wait(RecoveryDecisionReason::PressureClearancePending)
        );
        assert_eq!(
            evaluate_recovery(
                context(),
                RecoveryCause::ResourceExhausted,
                RecoveryEvidence {
                    pressure_cleared: Some(true),
                    ..pressure_present
                }
            ),
            RecoveryDecision::PermitReplacement(RecoveryDecisionReason::PressureCleared)
        );
    }

    #[test]
    fn unsupported_format_and_internal_failure_remain_stopped() {
        for (cause, reason) in [
            (
                RecoveryCause::UnsupportedFormat,
                RecoveryDecisionReason::UnsupportedFormat,
            ),
            (
                RecoveryCause::InternalFailure,
                RecoveryDecisionReason::InternalFailure,
            ),
        ] {
            let evidence = RecoveryEvidence {
                retry_hint: Some(RetryHint::ChangeFormat),
                ..completed_stream()
            };
            assert_eq!(
                evaluate_recovery(context(), cause, evidence),
                RecoveryDecision::RemainStopped(reason)
            );
        }
    }

    #[test]
    fn startup_failure_and_worker_panic_are_not_retryable() {
        assert_eq!(
            evaluate_recovery(
                context(),
                RecoveryCause::StartupFailure,
                completed_startup_attempt()
            ),
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::StartupFailure)
        );
        assert_eq!(
            evaluate_recovery(
                context(),
                RecoveryCause::WorkerPanic,
                completed_startup_attempt()
            ),
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::WorkerPanic)
        );
    }

    #[test]
    fn retry_hint_is_a_constraint_not_authorization() {
        let evidence = RecoveryEvidence {
            retry_hint: Some(RetryHint::DoNotRetry),
            attempts_remaining: Some(true),
            ..completed_stream()
        };
        assert_eq!(
            evaluate_recovery(context(), RecoveryCause::Interrupted, evidence),
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::RetryVetoed)
        );
    }

    #[test]
    fn successful_eligibility_is_only_a_value() {
        let evidence = RecoveryEvidence {
            retry_hint: Some(RetryHint::RetryNow),
            attempts_remaining: Some(true),
            ..completed_stream()
        };

        let decision = evaluate_recovery(context(), RecoveryCause::Interrupted, evidence);

        assert_eq!(
            decision,
            RecoveryDecision::PermitReplacement(RecoveryDecisionReason::RetryEligible)
        );
        assert_eq!(evidence.owner_completed, Some(true));
        assert_eq!(evidence.resources_released, Some(true));
    }

    #[test]
    fn configuration_identity_mismatch_fails_closed_before_authorization() {
        let configuration_a = enabled_configuration(11, 3);
        let configuration_b = enabled_configuration(12, 3);
        let state = failed_state(&configuration_a, RetryFailureCause::Interrupted);
        let snapshot = state.snapshot().expect("failed state has a snapshot");

        assert_eq!(
            evaluate_recovery_policy(
                &configuration_a,
                configuration_b.identity(),
                &snapshot,
                policy_context(&snapshot),
                RecoveryCause::Interrupted,
                policy_evidence(RetryHint::RetryNow),
            ),
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::StaleConfiguration)
        );

        let mut mismatched_state = snapshot.clone();
        mismatched_state.configuration_id = configuration_b.identity();
        assert_eq!(
            evaluate_recovery_policy(
                &configuration_a,
                configuration_a.identity(),
                &mismatched_state,
                policy_context(&mismatched_state),
                RecoveryCause::Interrupted,
                policy_evidence(RetryHint::RetryNow),
            ),
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::StaleConfiguration)
        );
    }

    #[test]
    fn configured_budget_overrides_caller_supplied_attempt_claims() {
        let configuration = enabled_configuration(21, 3);
        let mut state = failed_state(&configuration, RetryFailureCause::Interrupted);

        for _ in 0..3 {
            let evaluated = state.snapshot().expect("waiting state has a snapshot");
            let authorization = evaluated
                .authorize(RecoveryDecision::PermitReplacement(
                    RecoveryDecisionReason::RetryEligible,
                ))
                .expect("test decision permits state accounting");
            let attempt = state
                .commit_recovery_attempt(&authorization)
                .expect("current authorization commits one automatic attempt");
            state
                .record_failure(attempt, RetryFailureCause::Interrupted)
                .expect("automatic attempt failure is recorded");
            state
                .record_cleanup_complete(attempt)
                .expect("automatic attempt cleanup is recorded");
        }

        let exhausted = state.snapshot().expect("failed state has a snapshot");
        assert_eq!(
            exhausted
                .recovery_episode
                .expect("failure creates an episode")
                .automatic_recovery_attempts_started,
            3
        );
        assert_eq!(
            evaluate_recovery_policy(
                &configuration,
                configuration.identity(),
                &exhausted,
                policy_context(&exhausted),
                RecoveryCause::Interrupted,
                policy_evidence(RetryHint::RetryNow),
            ),
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::RetryBudgetExhausted)
        );
    }

    #[test]
    fn configured_cooldown_requires_matching_state_evidence() {
        let configuration = enabled_configuration(31, 3);
        let mut state = failed_state(&configuration, RetryFailureCause::Interrupted);
        let pending = state.snapshot().expect("failed state has a snapshot");

        assert_eq!(
            evaluate_recovery_policy(
                &configuration,
                configuration.identity(),
                &pending,
                policy_context(&pending),
                RecoveryCause::Interrupted,
                policy_evidence(RetryHint::RetryNow),
            ),
            RecoveryDecision::Wait(RecoveryDecisionReason::CooldownPending)
        );

        satisfy_test_cooldown(&mut state, 31);
        let satisfied = state.snapshot().expect("cooldown state has a snapshot");
        let decision = evaluate_recovery_policy(
            &configuration,
            configuration.identity(),
            &satisfied,
            policy_context(&satisfied),
            RecoveryCause::Interrupted,
            policy_evidence(RetryHint::RetryNow),
        );
        assert_eq!(
            decision,
            RecoveryDecision::PermitReplacement(RecoveryDecisionReason::RetryEligible)
        );
        assert_eq!(
            state.snapshot().expect("policy leaves state inspectable"),
            satisfied
        );
    }

    #[test]
    fn failure_classification_controls_retryable_and_guarded_causes() {
        let enabled = enabled_configuration(41, 3);
        let disabled = disabled_configuration(42);

        let disabled_interruption = failed_state(&disabled, RetryFailureCause::Interrupted);
        let disabled_snapshot = disabled_interruption
            .snapshot()
            .expect("disabled state has a snapshot");
        assert_eq!(
            evaluate_recovery_policy(
                &disabled,
                disabled.identity(),
                &disabled_snapshot,
                policy_context(&disabled_snapshot),
                RecoveryCause::Interrupted,
                policy_evidence(RetryHint::RetryNow),
            ),
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::ConfiguredNonRetryable)
        );

        let disabled_panic = failed_state(&disabled, RetryFailureCause::WorkerPanic);
        let panic_snapshot = disabled_panic
            .snapshot()
            .expect("panic state has a snapshot");
        assert_eq!(
            evaluate_recovery_policy(
                &disabled,
                disabled.identity(),
                &panic_snapshot,
                policy_context(&panic_snapshot),
                RecoveryCause::WorkerPanic,
                completed_startup_attempt(),
            ),
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::ConfiguredNonRetryable)
        );

        let mut device = failed_state(
            &enabled,
            RetryFailureCause::DeviceUnavailable(DeviceUnavailableCause::Removed),
        );
        satisfy_test_cooldown(&mut device, 41);
        let device_snapshot = device.snapshot().expect("device state has a snapshot");
        let unavailable = RecoveryEvidence {
            retry_hint: Some(RetryHint::WaitForSource),
            source_policy: Some(RecoverySourcePolicy::Pinned),
            source_available: Some(false),
            ..completed_startup_attempt()
        };
        assert_eq!(
            evaluate_recovery_policy(
                &enabled,
                enabled.identity(),
                &device_snapshot,
                policy_context(&device_snapshot),
                RecoveryCause::DeviceUnavailable(DeviceUnavailableCause::Removed),
                unavailable,
            ),
            RecoveryDecision::Wait(RecoveryDecisionReason::SourceUnavailable)
        );
        assert_eq!(
            evaluate_recovery_policy(
                &enabled,
                enabled.identity(),
                &device_snapshot,
                policy_context(&device_snapshot),
                RecoveryCause::DeviceUnavailable(DeviceUnavailableCause::Removed),
                RecoveryEvidence {
                    source_available: Some(true),
                    ..unavailable
                },
            ),
            RecoveryDecision::PermitReplacement(RecoveryDecisionReason::SourceAvailable)
        );

        let mut unsupported = failed_state(&enabled, RetryFailureCause::UnsupportedFormat);
        satisfy_test_cooldown(&mut unsupported, 42);
        let unsupported_snapshot = unsupported
            .snapshot()
            .expect("unsupported-format state has a snapshot");
        assert_eq!(
            evaluate_recovery_policy(
                &enabled,
                enabled.identity(),
                &unsupported_snapshot,
                policy_context(&unsupported_snapshot),
                RecoveryCause::UnsupportedFormat,
                RecoveryEvidence {
                    retry_hint: Some(RetryHint::ChangeFormat),
                    supported_format_available: Some(true),
                    ..completed_startup_attempt()
                },
            ),
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::UnsupportedFormat)
        );
    }

    #[test]
    fn invalid_reset_evidence_cannot_clear_exhaustion_but_new_intent_does() {
        let configuration = enabled_configuration(51, 1);
        let mut state = failed_state(&configuration, RetryFailureCause::Interrupted);
        let evaluated = state.snapshot().expect("waiting state has a snapshot");
        state
            .mark_exhausted(&evaluated, ExhaustionReason::AutomaticRecoveryBudget)
            .expect("current episode can become exhausted");
        let exhausted = state.snapshot().expect("exhausted state has a snapshot");

        assert!(state
            .advance_recovery_episode(
                exhausted.intent_generation,
                RecoveryEpisodeResetEvidence::StableRun(ResetEvidenceId(1)),
            )
            .is_err());
        assert_eq!(
            state.snapshot().expect("state remains inspectable"),
            exhausted
        );
        assert_eq!(
            evaluate_recovery_policy(
                &configuration,
                configuration.identity(),
                &exhausted,
                policy_context(&exhausted),
                RecoveryCause::Interrupted,
                RecoveryEvidence {
                    stream_started: Some(true),
                    terminal_event_delivered: Some(true),
                    owner_completed: Some(true),
                    resources_released: Some(true),
                    retry_hint: Some(RetryHint::RetryNow),
                    ..RecoveryEvidence::default()
                },
            ),
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::RetryBudgetExhausted)
        );

        state
            .explicit_stop(exhausted.intent_generation)
            .expect("explicit stop invalidates the exhausted intent");
        let new_generation = state
            .explicit_start(configuration.identity())
            .expect("explicit start creates fresh intent state");
        let attempt = state
            .commit_initial_attempt(new_generation)
            .expect("new intent commits its initial attempt");
        state
            .record_failure(attempt, RetryFailureCause::Interrupted)
            .expect("new intent failure is recorded");
        state
            .record_cleanup_complete(attempt)
            .expect("new intent cleanup is recorded");
        satisfy_test_cooldown(&mut state, 51);
        let reset = state.snapshot().expect("new intent has a snapshot");
        assert_ne!(reset.intent_generation, exhausted.intent_generation);
        assert_eq!(
            evaluate_recovery_policy(
                &configuration,
                configuration.identity(),
                &reset,
                policy_context(&reset),
                RecoveryCause::Interrupted,
                policy_evidence(RetryHint::RetryNow),
            ),
            RecoveryDecision::PermitReplacement(RecoveryDecisionReason::RetryEligible)
        );
    }
}
