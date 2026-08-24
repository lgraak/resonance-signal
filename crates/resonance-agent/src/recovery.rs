//! Side-effect-free recovery policy evaluation.
//!
//! This module represents policy authorization only. It does not create or
//! control capture owners, wait for time or hardware, or mutate retry state.

// Milestone 6E deliberately represents policy without wiring it into runtime
// orchestration. The types become live when separately approved enforcement is
// added to CaptureSupervisor.
#![allow(dead_code)]

use resonance_api::contract::RetryHint;

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
    CleanupPending,
    TerminalBoundaryPending,
    MissingEvidence,
    InconsistentEvidence,
    RetryVetoed,
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
    UnsupportedFormat,
    InternalFailure,
    StartupFailure,
    WorkerPanic,
}

/// Evaluates recovery authorization without performing recovery behavior.
pub(crate) fn evaluate_recovery(
    context: RecoveryContext,
    cause: RecoveryCause,
    evidence: RecoveryEvidence,
) -> RecoveryDecision {
    if !context.desired_running || cause == RecoveryCause::ExplicitStop {
        return RecoveryDecision::RemainStopped(RecoveryDecisionReason::ExplicitStop);
    }

    if context.evaluated_intent_generation != context.current_intent_generation {
        return RecoveryDecision::RemainStopped(RecoveryDecisionReason::StaleIntent);
    }

    let Some(stream_started) = evidence.stream_started else {
        return missing_evidence();
    };
    let Some(owner_completed) = evidence.owner_completed else {
        return missing_evidence();
    };
    let Some(resources_released) = evidence.resources_released else {
        return missing_evidence();
    };

    if !owner_completed || !resources_released {
        return RecoveryDecision::Wait(RecoveryDecisionReason::CleanupPending);
    }

    if stream_started {
        let Some(terminal_event_delivered) = evidence.terminal_event_delivered else {
            return missing_evidence();
        };
        if !terminal_event_delivered {
            return RecoveryDecision::Wait(RecoveryDecisionReason::TerminalBoundaryPending);
        }
    } else if evidence.terminal_event_delivered == Some(true) {
        return RecoveryDecision::RemainStopped(RecoveryDecisionReason::InconsistentEvidence);
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
}
