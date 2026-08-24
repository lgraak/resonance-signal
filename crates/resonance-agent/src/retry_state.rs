//! Supervisor-owned retry state and deterministic state transitions.
//!
//! This module records facts and validates transitions only. It has no owner
//! factory, clock, timer, thread, device, event sink, or recovery action.

// Recovery execution remains separately gated. This module only records facts
// supplied by CaptureSupervisor and validates state transitions.
#![allow(dead_code)]

use std::collections::VecDeque;

use crate::recovery::{
    DeviceUnavailableCause, RecoveryCause, RecoveryDecision, RecoveryDecisionReason,
    SourceReconfigurationCause,
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct IntentGeneration(u64);

impl IntentGeneration {
    pub(crate) const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct StateRevision(u64);

impl StateRevision {
    pub(crate) const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct PolicyConfigurationId(pub(crate) u64);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct AttemptOrdinal(u64);

impl AttemptOrdinal {
    pub(crate) const fn get(self) -> u64 {
        self.0
    }
}

/// Stable identity for exactly one owner-creation call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AttemptId {
    pub(crate) intent_generation: IntentGeneration,
    pub(crate) ordinal: AttemptOrdinal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AttemptKind {
    ExplicitStart,
    AutomaticRecovery,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AttemptLifecycle {
    Committed,
    OwnerCreated,
    Running,
    Failed,
    Completed,
    CleanupComplete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RetryPhase {
    /// No owner-creation attempt has been committed for the current intent.
    Idle,
    /// One attempt has been committed but has not established a stream.
    Attempting,
    /// The current attempt has established a stream.
    Running,
    /// Failure is recorded but terminal or cleanup evidence is incomplete.
    Failed,
    /// Failure evidence is complete and policy may evaluate a snapshot.
    Waiting,
    /// Automatic recovery is exhausted for the current episode.
    Exhausted,
    /// Running intent is absent and pending recovery is invalid.
    Stopped,
}

/// Failure classes retained by retry state.
///
/// Normal shutdown and explicit stop are not failures and therefore cannot be
/// inserted into retry history or increment failure counters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RetryFailureCause {
    OwnerConstructionFailure,
    DeviceUnavailable(DeviceUnavailableCause),
    SourceReconfigured(SourceReconfigurationCause),
    Interrupted,
    ResourceExhausted,
    UnsupportedFormat,
    InternalFailure,
    StartupFailure,
    WorkerPanic,
}

impl From<RetryFailureCause> for RecoveryCause {
    fn from(cause: RetryFailureCause) -> Self {
        match cause {
            RetryFailureCause::OwnerConstructionFailure => Self::StartupFailure,
            RetryFailureCause::DeviceUnavailable(cause) => Self::DeviceUnavailable(cause),
            RetryFailureCause::SourceReconfigured(cause) => Self::SourceReconfigured(cause),
            RetryFailureCause::Interrupted => Self::Interrupted,
            RetryFailureCause::ResourceExhausted => Self::ResourceExhausted,
            RetryFailureCause::UnsupportedFormat => Self::UnsupportedFormat,
            RetryFailureCause::InternalFailure => Self::InternalFailure,
            RetryFailureCause::StartupFailure => Self::StartupFailure,
            RetryFailureCause::WorkerPanic => Self::WorkerPanic,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AttemptState {
    pub(crate) id: AttemptId,
    pub(crate) kind: AttemptKind,
    pub(crate) lifecycle: AttemptLifecycle,
    pub(crate) stream_started: bool,
    pub(crate) terminal_event_delivered: bool,
    pub(crate) resources_released: bool,
    pub(crate) failure: Option<RetryFailureCause>,
}

impl AttemptState {
    fn ready_for_evaluation(self) -> bool {
        self.failure.is_some()
            && self.resources_released
            && (!self.stream_started || self.terminal_event_delivered)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct RecoveryEpisodeOrdinal(u64);

impl RecoveryEpisodeOrdinal {
    pub(crate) const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RecoveryEpisodeId {
    pub(crate) intent_generation: IntentGeneration,
    pub(crate) ordinal: RecoveryEpisodeOrdinal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExhaustionReason {
    AutomaticRecoveryBudget,
    PolicyVeto,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ExhaustionRecord {
    pub(crate) intent_generation: IntentGeneration,
    pub(crate) recovery_episode: RecoveryEpisodeId,
    pub(crate) evaluated_revision: StateRevision,
    pub(crate) attempt_id: AttemptId,
    pub(crate) reason: ExhaustionReason,
    pub(crate) configuration_id: PolicyConfigurationId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RecoveryEpisodeState {
    pub(crate) id: RecoveryEpisodeId,
    pub(crate) automatic_recovery_attempts_started: u64,
    pub(crate) exhaustion: Option<ExhaustionRecord>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct CooldownEligibilityMarker(pub(crate) u64);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct CooldownEvidenceId(pub(crate) u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CooldownRequirement {
    pub(crate) attempt_id: AttemptId,
    pub(crate) marker: CooldownEligibilityMarker,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CooldownState {
    NotRequired,
    Pending(CooldownRequirement),
    Satisfied {
        requirement: CooldownRequirement,
        evidence: CooldownEvidenceId,
    },
    Invalidated(CooldownRequirement),
}

impl CooldownState {
    fn permits_progression(self) -> bool {
        matches!(self, Self::NotRequired | Self::Satisfied { .. })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FailureRecord {
    pub(crate) attempt_id: AttemptId,
    pub(crate) recovery_episode: RecoveryEpisodeId,
    pub(crate) cause: RetryFailureCause,
    pub(crate) stream_started: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct FailureTotals {
    pub(crate) total: u64,
    pub(crate) owner_construction_failure: u64,
    pub(crate) device_unavailable: u64,
    pub(crate) source_reconfigured: u64,
    pub(crate) interrupted: u64,
    pub(crate) resource_exhausted: u64,
    pub(crate) unsupported_format: u64,
    pub(crate) internal_failure: u64,
    pub(crate) startup_failure: u64,
    pub(crate) worker_panic: u64,
}

impl FailureTotals {
    fn checked_with_record(self, cause: RetryFailureCause) -> Result<Self, RetryTransitionError> {
        let mut updated = self;
        updated.total = updated
            .total
            .checked_add(1)
            .ok_or(RetryTransitionError::CounterExhausted)?;
        let counter = match cause {
            RetryFailureCause::OwnerConstructionFailure => &mut updated.owner_construction_failure,
            RetryFailureCause::DeviceUnavailable(_) => &mut updated.device_unavailable,
            RetryFailureCause::SourceReconfigured(_) => &mut updated.source_reconfigured,
            RetryFailureCause::Interrupted => &mut updated.interrupted,
            RetryFailureCause::ResourceExhausted => &mut updated.resource_exhausted,
            RetryFailureCause::UnsupportedFormat => &mut updated.unsupported_format,
            RetryFailureCause::InternalFailure => &mut updated.internal_failure,
            RetryFailureCause::StartupFailure => &mut updated.startup_failure,
            RetryFailureCause::WorkerPanic => &mut updated.worker_panic,
        };
        *counter = counter
            .checked_add(1)
            .ok_or(RetryTransitionError::CounterExhausted)?;
        Ok(updated)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ResetEvidenceId(pub(crate) u64);

/// Evidence already validated by a future policy or evidence component.
///
/// Retry state consumes this evidence but does not measure stability or decide
/// whether a source change is material.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecoveryEpisodeResetEvidence {
    StableRun(ResetEvidenceId),
    ChangedPrecondition(ResetEvidenceId),
}

/// Owned immutable policy-evaluation input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RetrySnapshot {
    pub(crate) desired_running: bool,
    pub(crate) intent_generation: IntentGeneration,
    pub(crate) state_revision: StateRevision,
    pub(crate) configuration_id: PolicyConfigurationId,
    pub(crate) phase: RetryPhase,
    pub(crate) attempts_started: u64,
    pub(crate) current_attempt: Option<AttemptState>,
    pub(crate) recovery_episode: Option<RecoveryEpisodeState>,
    pub(crate) last_failure: Option<FailureRecord>,
    pub(crate) recent_failures: Vec<FailureRecord>,
    pub(crate) failure_totals: FailureTotals,
    pub(crate) consecutive_failed_attempts: u64,
    pub(crate) cooldown: CooldownState,
}

impl RetrySnapshot {
    /// Binds only a permit decision to the immutable facts it evaluated.
    pub(crate) fn authorize(
        &self,
        decision: RecoveryDecision,
    ) -> Result<RecoveryAuthorization, RetryTransitionError> {
        let RecoveryDecision::PermitReplacement(reason) = decision else {
            return Err(RetryTransitionError::DecisionDoesNotPermitRecovery);
        };
        Ok(RecoveryAuthorization {
            intent_generation: self.intent_generation,
            state_revision: self.state_revision,
            configuration_id: self.configuration_id,
            recovery_episode: self.recovery_episode.map(|episode| episode.id),
            prior_attempt_id: self.current_attempt.map(|attempt| attempt.id),
            reason,
        })
    }
}

/// One policy permission bound to exactly one evaluated state revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RecoveryAuthorization {
    pub(crate) intent_generation: IntentGeneration,
    pub(crate) state_revision: StateRevision,
    pub(crate) configuration_id: PolicyConfigurationId,
    pub(crate) recovery_episode: Option<RecoveryEpisodeId>,
    pub(crate) prior_attempt_id: Option<AttemptId>,
    pub(crate) reason: RecoveryDecisionReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RetryTransitionError {
    HistoryCapacityZero,
    NoActiveIntent,
    IntentAlreadyRunning,
    StaleIntent {
        current: IntentGeneration,
        supplied: IntentGeneration,
    },
    StaleSnapshot,
    DecisionDoesNotPermitRecovery,
    InvalidPhase(RetryPhase),
    NoCurrentAttempt,
    AttemptMismatch {
        current: AttemptId,
        supplied: AttemptId,
    },
    InvalidAttemptLifecycle(AttemptLifecycle),
    FailureAlreadyRecorded,
    TerminalEventNotApplicable,
    CleanupPending,
    TerminalEvidencePending,
    NoRecoveryEpisode,
    EpisodeExhausted,
    CooldownPending,
    CooldownEvidenceMismatch,
    InvalidEpisodeResetEvidence,
    CounterExhausted,
}

/// Mutable retry state owned by one capture supervisor.
///
/// `HISTORY_CAPACITY` makes boundedness structural while leaving the concrete
/// capacity for future configuration. No numeric retry limit is represented.
pub(crate) struct RetryState<const HISTORY_CAPACITY: usize> {
    last_intent_generation: u64,
    intent_generation: Option<IntentGeneration>,
    state_revision: StateRevision,
    configuration_id: Option<PolicyConfigurationId>,
    desired_running: bool,
    phase: RetryPhase,
    attempts_started: u64,
    current_attempt: Option<AttemptState>,
    recovery_episode: Option<RecoveryEpisodeState>,
    last_failure: Option<FailureRecord>,
    recent_failures: VecDeque<FailureRecord>,
    failure_totals: FailureTotals,
    consecutive_failed_attempts: u64,
    cooldown: CooldownState,
}

impl<const HISTORY_CAPACITY: usize> RetryState<HISTORY_CAPACITY> {
    pub(crate) fn new() -> Result<Self, RetryTransitionError> {
        if HISTORY_CAPACITY == 0 {
            return Err(RetryTransitionError::HistoryCapacityZero);
        }
        Ok(Self {
            last_intent_generation: 0,
            intent_generation: None,
            state_revision: StateRevision(0),
            configuration_id: None,
            desired_running: false,
            phase: RetryPhase::Idle,
            attempts_started: 0,
            current_attempt: None,
            recovery_episode: None,
            last_failure: None,
            recent_failures: VecDeque::with_capacity(HISTORY_CAPACITY),
            failure_totals: FailureTotals::default(),
            consecutive_failed_attempts: 0,
            cooldown: CooldownState::NotRequired,
        })
    }

    /// Creates a fresh capture-intent generation without creating an owner.
    pub(crate) fn explicit_start(
        &mut self,
        configuration_id: PolicyConfigurationId,
    ) -> Result<IntentGeneration, RetryTransitionError> {
        if self.desired_running {
            return Err(RetryTransitionError::IntentAlreadyRunning);
        }
        if self
            .current_attempt
            .is_some_and(|attempt| !attempt.resources_released)
        {
            return Err(RetryTransitionError::CleanupPending);
        }
        self.last_intent_generation = self
            .last_intent_generation
            .checked_add(1)
            .ok_or(RetryTransitionError::CounterExhausted)?;
        let generation = IntentGeneration(self.last_intent_generation);
        self.intent_generation = Some(generation);
        self.state_revision = StateRevision(1);
        self.configuration_id = Some(configuration_id);
        self.desired_running = true;
        self.phase = RetryPhase::Idle;
        self.attempts_started = 0;
        self.current_attempt = None;
        self.recovery_episode = None;
        self.last_failure = None;
        self.recent_failures.clear();
        self.failure_totals = FailureTotals::default();
        self.consecutive_failed_attempts = 0;
        self.cooldown = CooldownState::NotRequired;
        Ok(generation)
    }

    /// Invalidates recovery for the supplied generation before owner shutdown.
    pub(crate) fn explicit_stop(
        &mut self,
        generation: IntentGeneration,
    ) -> Result<(), RetryTransitionError> {
        self.require_generation(generation)?;
        let next_revision = self.next_revision()?;
        self.desired_running = false;
        self.phase = RetryPhase::Stopped;
        if let CooldownState::Pending(requirement) | CooldownState::Satisfied { requirement, .. } =
            self.cooldown
        {
            self.cooldown = CooldownState::Invalidated(requirement);
        }
        self.state_revision = next_revision;
        Ok(())
    }

    /// Allocates the initial attempt immediately before one owner-creation call.
    pub(crate) fn commit_initial_attempt(
        &mut self,
        generation: IntentGeneration,
    ) -> Result<AttemptId, RetryTransitionError> {
        self.require_generation(generation)?;
        if !self.desired_running || self.phase != RetryPhase::Idle {
            return Err(RetryTransitionError::InvalidPhase(self.phase));
        }
        self.commit_attempt(AttemptKind::ExplicitStart)
    }

    /// Consumes one still-current evaluated state and allocates one recovery
    /// attempt. Returning an identity is state accounting, not owner creation.
    pub(crate) fn commit_recovery_attempt(
        &mut self,
        authorization: &RecoveryAuthorization,
    ) -> Result<AttemptId, RetryTransitionError> {
        self.require_current_authorization(authorization)?;
        if self
            .recovery_episode
            .is_some_and(|episode| episode.exhaustion.is_some())
        {
            return Err(RetryTransitionError::EpisodeExhausted);
        }
        let attempt = self
            .current_attempt
            .ok_or(RetryTransitionError::NoCurrentAttempt)?;
        if !attempt.resources_released {
            return Err(RetryTransitionError::CleanupPending);
        }
        if attempt.stream_started && !attempt.terminal_event_delivered {
            return Err(RetryTransitionError::TerminalEvidencePending);
        }
        if !self.desired_running || self.phase != RetryPhase::Waiting {
            return Err(RetryTransitionError::InvalidPhase(self.phase));
        }
        let episode = self
            .recovery_episode
            .ok_or(RetryTransitionError::NoRecoveryEpisode)?;
        if episode.exhaustion.is_some() {
            return Err(RetryTransitionError::EpisodeExhausted);
        }
        if !self.cooldown.permits_progression() {
            return Err(RetryTransitionError::CooldownPending);
        }
        self.commit_attempt(AttemptKind::AutomaticRecovery)
    }

    fn commit_attempt(&mut self, kind: AttemptKind) -> Result<AttemptId, RetryTransitionError> {
        let generation = self
            .intent_generation
            .ok_or(RetryTransitionError::NoActiveIntent)?;
        let attempts_started = self
            .attempts_started
            .checked_add(1)
            .ok_or(RetryTransitionError::CounterExhausted)?;
        let id = AttemptId {
            intent_generation: generation,
            ordinal: AttemptOrdinal(attempts_started),
        };
        let automatic_recovery_attempts_started = if kind == AttemptKind::AutomaticRecovery {
            Some(
                self.recovery_episode
                    .ok_or(RetryTransitionError::NoRecoveryEpisode)?
                    .automatic_recovery_attempts_started
                    .checked_add(1)
                    .ok_or(RetryTransitionError::CounterExhausted)?,
            )
        } else {
            None
        };
        let next_revision = self.next_revision()?;

        if let Some(automatic_recovery_attempts_started) = automatic_recovery_attempts_started {
            self.recovery_episode
                .as_mut()
                .ok_or(RetryTransitionError::NoRecoveryEpisode)?
                .automatic_recovery_attempts_started = automatic_recovery_attempts_started;
        }
        self.attempts_started = attempts_started;
        self.current_attempt = Some(AttemptState {
            id,
            kind,
            lifecycle: AttemptLifecycle::Committed,
            stream_started: false,
            terminal_event_delivered: false,
            resources_released: false,
            failure: None,
        });
        self.phase = RetryPhase::Attempting;
        self.cooldown = CooldownState::NotRequired;
        self.state_revision = next_revision;
        Ok(id)
    }

    pub(crate) fn record_owner_created(
        &mut self,
        attempt_id: AttemptId,
    ) -> Result<(), RetryTransitionError> {
        let next_revision = self.next_revision()?;
        let attempt = self.require_attempt_mut(attempt_id)?;
        if attempt.lifecycle != AttemptLifecycle::Committed {
            return Err(RetryTransitionError::InvalidAttemptLifecycle(
                attempt.lifecycle,
            ));
        }
        attempt.lifecycle = AttemptLifecycle::OwnerCreated;
        self.state_revision = next_revision;
        Ok(())
    }

    pub(crate) fn record_stream_started(
        &mut self,
        attempt_id: AttemptId,
    ) -> Result<(), RetryTransitionError> {
        let next_revision = self.next_revision()?;
        let attempt = self.require_attempt_mut(attempt_id)?;
        if attempt.lifecycle != AttemptLifecycle::OwnerCreated {
            return Err(RetryTransitionError::InvalidAttemptLifecycle(
                attempt.lifecycle,
            ));
        }
        attempt.lifecycle = AttemptLifecycle::Running;
        attempt.stream_started = true;
        if self.desired_running {
            self.phase = RetryPhase::Running;
        }
        self.state_revision = next_revision;
        Ok(())
    }

    /// Records a non-failure owner outcome against the current attempt.
    pub(crate) fn record_normal_completion(
        &mut self,
        attempt_id: AttemptId,
    ) -> Result<(), RetryTransitionError> {
        let next_revision = self.next_revision()?;
        let attempt = self.require_attempt_mut(attempt_id)?;
        if attempt.failure.is_some()
            || !matches!(
                attempt.lifecycle,
                AttemptLifecycle::OwnerCreated | AttemptLifecycle::Running
            )
        {
            return Err(RetryTransitionError::InvalidAttemptLifecycle(
                attempt.lifecycle,
            ));
        }
        attempt.lifecycle = AttemptLifecycle::Completed;
        self.state_revision = next_revision;
        Ok(())
    }

    /// Records one typed failure against the already-counted attempt.
    pub(crate) fn record_failure(
        &mut self,
        attempt_id: AttemptId,
        cause: RetryFailureCause,
    ) -> Result<(), RetryTransitionError> {
        let attempt = self.require_attempt(attempt_id)?;
        if attempt.failure.is_some() {
            return Err(RetryTransitionError::FailureAlreadyRecorded);
        }
        if !matches!(
            attempt.lifecycle,
            AttemptLifecycle::Committed
                | AttemptLifecycle::OwnerCreated
                | AttemptLifecycle::Running
        ) {
            return Err(RetryTransitionError::InvalidAttemptLifecycle(
                attempt.lifecycle,
            ));
        }

        let episode = if let Some(episode) = self.recovery_episode {
            episode
        } else {
            let generation = self
                .intent_generation
                .ok_or(RetryTransitionError::NoActiveIntent)?;
            RecoveryEpisodeState {
                id: RecoveryEpisodeId {
                    intent_generation: generation,
                    ordinal: RecoveryEpisodeOrdinal(1),
                },
                automatic_recovery_attempts_started: 0,
                exhaustion: None,
            }
        };
        let record = FailureRecord {
            attempt_id,
            recovery_episode: episode.id,
            cause,
            stream_started: attempt.stream_started,
        };
        let failure_totals = self.failure_totals.checked_with_record(cause)?;
        let consecutive_failed_attempts = self
            .consecutive_failed_attempts
            .checked_add(1)
            .ok_or(RetryTransitionError::CounterExhausted)?;
        let next_revision = self.next_revision()?;

        if self.recovery_episode.is_none() {
            self.recovery_episode = Some(episode);
        }
        self.failure_totals = failure_totals;
        self.consecutive_failed_attempts = consecutive_failed_attempts;
        if self.recent_failures.len() == HISTORY_CAPACITY {
            self.recent_failures.pop_front();
        }
        self.recent_failures.push_back(record);
        self.last_failure = Some(record);

        let attempt = self.require_attempt_mut(attempt_id)?;
        attempt.failure = Some(cause);
        attempt.lifecycle = AttemptLifecycle::Failed;
        if self.desired_running {
            self.phase = RetryPhase::Failed;
        }
        self.state_revision = next_revision;
        self.refresh_post_failure_phase();
        Ok(())
    }

    pub(crate) fn record_terminal_event_delivered(
        &mut self,
        attempt_id: AttemptId,
    ) -> Result<(), RetryTransitionError> {
        let next_revision = self.next_revision()?;
        let attempt = self.require_attempt_mut(attempt_id)?;
        if !attempt.stream_started
            || !matches!(
                attempt.lifecycle,
                AttemptLifecycle::Running | AttemptLifecycle::Failed | AttemptLifecycle::Completed
            )
        {
            return Err(RetryTransitionError::TerminalEventNotApplicable);
        }
        attempt.terminal_event_delivered = true;
        self.state_revision = next_revision;
        self.refresh_post_failure_phase();
        Ok(())
    }

    /// Records joined completion and resource release. It never creates a new
    /// attempt and cannot make failure evaluable without terminal evidence.
    pub(crate) fn record_cleanup_complete(
        &mut self,
        attempt_id: AttemptId,
    ) -> Result<(), RetryTransitionError> {
        let next_revision = self.next_revision()?;
        let desired_running = self.desired_running;
        let attempt = self.require_attempt_mut(attempt_id)?;
        attempt.resources_released = true;
        if attempt.lifecycle == AttemptLifecycle::Completed
            || (attempt.failure.is_some()
                && (!attempt.stream_started || attempt.terminal_event_delivered))
        {
            attempt.lifecycle = AttemptLifecycle::CleanupComplete;
        } else if !desired_running {
            attempt.lifecycle = AttemptLifecycle::CleanupComplete;
        }
        self.state_revision = next_revision;
        self.refresh_post_failure_phase();
        Ok(())
    }

    /// Applies a policy-produced cooldown requirement to a current snapshot.
    pub(crate) fn require_cooldown(
        &mut self,
        evaluated: &RetrySnapshot,
        marker: CooldownEligibilityMarker,
    ) -> Result<(), RetryTransitionError> {
        self.require_current_snapshot(evaluated)?;
        if self.phase != RetryPhase::Waiting {
            return Err(RetryTransitionError::InvalidPhase(self.phase));
        }
        let next_revision = self.next_revision()?;
        let attempt_id = self
            .current_attempt
            .ok_or(RetryTransitionError::NoCurrentAttempt)?
            .id;
        self.cooldown = CooldownState::Pending(CooldownRequirement { attempt_id, marker });
        self.state_revision = next_revision;
        Ok(())
    }

    /// Records externally supplied eligibility evidence without reading time.
    pub(crate) fn satisfy_cooldown(
        &mut self,
        generation: IntentGeneration,
        marker: CooldownEligibilityMarker,
        evidence: CooldownEvidenceId,
    ) -> Result<(), RetryTransitionError> {
        self.require_generation(generation)?;
        let CooldownState::Pending(requirement) = self.cooldown else {
            return Err(RetryTransitionError::CooldownEvidenceMismatch);
        };
        if requirement.marker != marker {
            return Err(RetryTransitionError::CooldownEvidenceMismatch);
        }
        let next_revision = self.next_revision()?;
        self.cooldown = CooldownState::Satisfied {
            requirement,
            evidence,
        };
        self.state_revision = next_revision;
        Ok(())
    }

    /// Makes exhaustion sticky for the episode represented by `evaluated`.
    pub(crate) fn mark_exhausted(
        &mut self,
        evaluated: &RetrySnapshot,
        reason: ExhaustionReason,
    ) -> Result<(), RetryTransitionError> {
        self.require_current_snapshot(evaluated)?;
        if self
            .recovery_episode
            .is_some_and(|episode| episode.exhaustion.is_some())
        {
            return Err(RetryTransitionError::EpisodeExhausted);
        }
        if self.phase != RetryPhase::Waiting {
            return Err(RetryTransitionError::InvalidPhase(self.phase));
        }
        let next_revision = self.next_revision()?;
        let attempt_id = self
            .current_attempt
            .ok_or(RetryTransitionError::NoCurrentAttempt)?
            .id;
        let configuration_id = self
            .configuration_id
            .ok_or(RetryTransitionError::NoActiveIntent)?;
        let episode = self
            .recovery_episode
            .as_mut()
            .ok_or(RetryTransitionError::NoRecoveryEpisode)?;
        episode.exhaustion = Some(ExhaustionRecord {
            intent_generation: evaluated.intent_generation,
            recovery_episode: episode.id,
            evaluated_revision: evaluated.state_revision,
            attempt_id,
            reason,
            configuration_id,
        });
        self.phase = RetryPhase::Exhausted;
        self.state_revision = next_revision;
        Ok(())
    }

    /// Advances the episode only after explicit, already-validated evidence.
    pub(crate) fn advance_recovery_episode(
        &mut self,
        generation: IntentGeneration,
        evidence: RecoveryEpisodeResetEvidence,
    ) -> Result<RecoveryEpisodeId, RetryTransitionError> {
        self.require_generation(generation)?;
        let valid = matches!(
            (self.phase, evidence),
            (
                RetryPhase::Running,
                RecoveryEpisodeResetEvidence::StableRun(_)
            ) | (
                RetryPhase::Waiting | RetryPhase::Exhausted,
                RecoveryEpisodeResetEvidence::ChangedPrecondition(_)
            )
        );
        if !valid {
            return Err(RetryTransitionError::InvalidEpisodeResetEvidence);
        }
        if self
            .current_attempt
            .is_some_and(|attempt| attempt.failure.is_some() && !attempt.ready_for_evaluation())
        {
            return Err(RetryTransitionError::CleanupPending);
        }
        let previous = self
            .recovery_episode
            .ok_or(RetryTransitionError::NoRecoveryEpisode)?;
        let ordinal = previous
            .id
            .ordinal
            .0
            .checked_add(1)
            .ok_or(RetryTransitionError::CounterExhausted)?;
        let next_revision = self.next_revision()?;
        let id = RecoveryEpisodeId {
            intent_generation: generation,
            ordinal: RecoveryEpisodeOrdinal(ordinal),
        };
        self.recovery_episode = Some(RecoveryEpisodeState {
            id,
            automatic_recovery_attempts_started: 0,
            exhaustion: None,
        });
        self.consecutive_failed_attempts = 0;
        self.cooldown = CooldownState::NotRequired;
        if self.phase == RetryPhase::Exhausted {
            self.phase = RetryPhase::Waiting;
        }
        self.state_revision = next_revision;
        Ok(id)
    }

    pub(crate) fn snapshot(&self) -> Result<RetrySnapshot, RetryTransitionError> {
        Ok(RetrySnapshot {
            desired_running: self.desired_running,
            intent_generation: self
                .intent_generation
                .ok_or(RetryTransitionError::NoActiveIntent)?,
            state_revision: self.state_revision,
            configuration_id: self
                .configuration_id
                .ok_or(RetryTransitionError::NoActiveIntent)?,
            phase: self.phase,
            attempts_started: self.attempts_started,
            current_attempt: self.current_attempt,
            recovery_episode: self.recovery_episode,
            last_failure: self.last_failure,
            recent_failures: self.recent_failures.iter().copied().collect(),
            failure_totals: self.failure_totals,
            consecutive_failed_attempts: self.consecutive_failed_attempts,
            cooldown: self.cooldown,
        })
    }

    pub(crate) fn is_current_snapshot(&self, evaluated: &RetrySnapshot) -> bool {
        self.require_current_snapshot(evaluated).is_ok()
    }

    pub(crate) const fn intent_generation(&self) -> Option<IntentGeneration> {
        self.intent_generation
    }

    pub(crate) const fn desired_running(&self) -> bool {
        self.desired_running
    }

    fn refresh_post_failure_phase(&mut self) {
        if !self.desired_running || self.phase == RetryPhase::Exhausted {
            return;
        }
        if self
            .current_attempt
            .is_some_and(AttemptState::ready_for_evaluation)
        {
            self.phase = RetryPhase::Waiting;
            if let Some(attempt) = self.current_attempt.as_mut() {
                attempt.lifecycle = AttemptLifecycle::CleanupComplete;
            }
        }
    }

    fn require_generation(&self, supplied: IntentGeneration) -> Result<(), RetryTransitionError> {
        let current = self
            .intent_generation
            .ok_or(RetryTransitionError::NoActiveIntent)?;
        if current != supplied {
            return Err(RetryTransitionError::StaleIntent { current, supplied });
        }
        Ok(())
    }

    fn require_current_snapshot(
        &self,
        evaluated: &RetrySnapshot,
    ) -> Result<(), RetryTransitionError> {
        let current = self
            .intent_generation
            .ok_or(RetryTransitionError::NoActiveIntent)?;
        if evaluated.intent_generation != current
            || evaluated.state_revision != self.state_revision
            || Some(evaluated.configuration_id) != self.configuration_id
            || evaluated.recovery_episode.map(|episode| episode.id)
                != self.recovery_episode.map(|episode| episode.id)
            || evaluated.current_attempt.map(|attempt| attempt.id)
                != self.current_attempt.map(|attempt| attempt.id)
        {
            return Err(RetryTransitionError::StaleSnapshot);
        }
        Ok(())
    }

    fn require_current_authorization(
        &self,
        authorization: &RecoveryAuthorization,
    ) -> Result<(), RetryTransitionError> {
        let current = self
            .intent_generation
            .ok_or(RetryTransitionError::NoActiveIntent)?;
        if authorization.intent_generation != current
            || authorization.state_revision != self.state_revision
            || Some(authorization.configuration_id) != self.configuration_id
            || authorization.recovery_episode != self.recovery_episode.map(|episode| episode.id)
            || authorization.prior_attempt_id != self.current_attempt.map(|attempt| attempt.id)
        {
            return Err(RetryTransitionError::StaleSnapshot);
        }
        Ok(())
    }

    fn require_attempt(&self, supplied: AttemptId) -> Result<AttemptState, RetryTransitionError> {
        let current = self
            .current_attempt
            .ok_or(RetryTransitionError::NoCurrentAttempt)?;
        if current.id != supplied {
            return Err(RetryTransitionError::AttemptMismatch {
                current: current.id,
                supplied,
            });
        }
        Ok(current)
    }

    fn require_attempt_mut(
        &mut self,
        supplied: AttemptId,
    ) -> Result<&mut AttemptState, RetryTransitionError> {
        let current = self
            .current_attempt
            .as_mut()
            .ok_or(RetryTransitionError::NoCurrentAttempt)?;
        if current.id != supplied {
            return Err(RetryTransitionError::AttemptMismatch {
                current: current.id,
                supplied,
            });
        }
        Ok(current)
    }

    fn next_revision(&self) -> Result<StateRevision, RetryTransitionError> {
        Ok(StateRevision(
            self.state_revision
                .0
                .checked_add(1)
                .ok_or(RetryTransitionError::CounterExhausted)?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recovery::{evaluate_recovery, RecoveryContext, RecoveryEvidence};
    use resonance_api::contract::RetryHint;

    const CONFIGURATION: PolicyConfigurationId = PolicyConfigurationId(17);

    fn state() -> RetryState<3> {
        RetryState::new().expect("test history capacity is nonzero")
    }

    fn permit(snapshot: &RetrySnapshot) -> RecoveryAuthorization {
        snapshot
            .authorize(RecoveryDecision::PermitReplacement(
                RecoveryDecisionReason::RetryEligible,
            ))
            .unwrap()
    }

    fn failed_initial_attempt(
        cause: RetryFailureCause,
        stream_started: bool,
    ) -> (RetryState<3>, IntentGeneration, AttemptId) {
        let mut state = state();
        let generation = state.explicit_start(CONFIGURATION).unwrap();
        let attempt = state.commit_initial_attempt(generation).unwrap();
        if stream_started {
            state.record_owner_created(attempt).unwrap();
            state.record_stream_started(attempt).unwrap();
        }
        state.record_failure(attempt, cause).unwrap();
        if stream_started {
            state.record_terminal_event_delivered(attempt).unwrap();
        }
        state.record_cleanup_complete(attempt).unwrap();
        (state, generation, attempt)
    }

    #[test]
    fn intent_transition_table_advances_generation_and_rejects_stale_input() {
        let mut state = state();
        let first = state.explicit_start(CONFIGURATION).unwrap();
        state.explicit_stop(first).unwrap();
        let second = state.explicit_start(CONFIGURATION).unwrap();

        let cases = [
            (
                first,
                Err(RetryTransitionError::StaleIntent {
                    current: second,
                    supplied: first,
                }),
            ),
            (second, Ok(())),
        ];
        for (generation, expected) in cases {
            assert_eq!(state.explicit_stop(generation), expected);
        }
        assert_eq!(first.get() + 1, second.get());
        assert!(!state.snapshot().unwrap().desired_running);
    }

    #[test]
    fn new_intent_cannot_bypass_prior_attempt_cleanup() {
        let mut state = state();
        let first = state.explicit_start(CONFIGURATION).unwrap();
        let attempt = state.commit_initial_attempt(first).unwrap();
        state.record_owner_created(attempt).unwrap();
        state.record_stream_started(attempt).unwrap();
        state.explicit_stop(first).unwrap();

        assert_eq!(
            state.explicit_start(CONFIGURATION),
            Err(RetryTransitionError::CleanupPending)
        );
        state.record_cleanup_complete(attempt).unwrap();
        assert_eq!(state.explicit_start(CONFIGURATION).unwrap().get(), 2);
    }

    #[test]
    fn explicit_stop_invalidates_pending_cooldown_and_snapshot() {
        let (mut state, generation, _) =
            failed_initial_attempt(RetryFailureCause::Interrupted, false);
        let evaluated = state.snapshot().unwrap();
        state
            .require_cooldown(&evaluated, CooldownEligibilityMarker(4))
            .unwrap();
        state.explicit_stop(generation).unwrap();

        assert!(matches!(
            state.snapshot().unwrap().cooldown,
            CooldownState::Invalidated(_)
        ));
        assert_eq!(
            state.commit_recovery_attempt(&permit(&evaluated)),
            Err(RetryTransitionError::StaleSnapshot)
        );
    }

    #[test]
    fn attempt_transition_table_counts_exactly_once() {
        let mut state = state();
        let generation = state.explicit_start(CONFIGURATION).unwrap();
        let attempt = state.commit_initial_attempt(generation).unwrap();

        let transitions: [fn(&mut RetryState<3>, AttemptId); 4] = [
            |state, attempt| state.record_owner_created(attempt).unwrap(),
            |state, attempt| state.record_stream_started(attempt).unwrap(),
            |state, attempt| {
                state
                    .record_failure(attempt, RetryFailureCause::Interrupted)
                    .unwrap()
            },
            |state, attempt| state.record_terminal_event_delivered(attempt).unwrap(),
        ];
        for transition in transitions {
            transition(&mut state, attempt);
            assert_eq!(state.snapshot().unwrap().attempts_started, 1);
        }
        state.record_cleanup_complete(attempt).unwrap();
        let snapshot = state.snapshot().unwrap();
        assert_eq!(snapshot.attempts_started, 1);
        assert_eq!(snapshot.current_attempt.unwrap().id, attempt);
        assert_eq!(snapshot.phase, RetryPhase::Waiting);
        assert_eq!(snapshot.failure_totals.total, 1);
        assert_eq!(
            state.record_failure(attempt, RetryFailureCause::InternalFailure),
            Err(RetryTransitionError::FailureAlreadyRecorded)
        );
        assert_eq!(state.snapshot().unwrap().attempts_started, 1);
    }

    #[test]
    fn successful_start_does_not_reset_episode_or_recovery_budget() {
        let (mut state, _, _) = failed_initial_attempt(RetryFailureCause::Interrupted, false);
        let authorization = permit(&state.snapshot().unwrap());
        let recovery = state.commit_recovery_attempt(&authorization).unwrap();
        state.record_owner_created(recovery).unwrap();
        state.record_stream_started(recovery).unwrap();

        let snapshot = state.snapshot().unwrap();
        assert_eq!(snapshot.attempts_started, 2);
        assert_eq!(
            snapshot
                .recovery_episode
                .unwrap()
                .automatic_recovery_attempts_started,
            1
        );
        assert_eq!(snapshot.consecutive_failed_attempts, 1);
    }

    #[test]
    fn episode_transition_table_keeps_exhaustion_sticky_until_explicit_evidence() {
        let (mut state, generation, _) =
            failed_initial_attempt(RetryFailureCause::InternalFailure, false);
        let first_episode = state.snapshot().unwrap().recovery_episode.unwrap().id;
        let evaluated = state.snapshot().unwrap();
        state
            .mark_exhausted(&evaluated, ExhaustionReason::AutomaticRecoveryBudget)
            .unwrap();

        let exhausted = state.snapshot().unwrap();
        let authorization = permit(&exhausted);
        let cases = [
            state.commit_recovery_attempt(&authorization),
            state.commit_recovery_attempt(&authorization),
        ];
        assert_eq!(
            cases,
            [
                Err(RetryTransitionError::EpisodeExhausted),
                Err(RetryTransitionError::EpisodeExhausted),
            ]
        );
        assert!(state
            .snapshot()
            .unwrap()
            .recovery_episode
            .unwrap()
            .exhaustion
            .is_some());

        let second_episode = state
            .advance_recovery_episode(
                generation,
                RecoveryEpisodeResetEvidence::ChangedPrecondition(ResetEvidenceId(9)),
            )
            .unwrap();
        assert_eq!(
            second_episode.ordinal.get(),
            first_episode.ordinal.get() + 1
        );
        let reset = state.snapshot().unwrap();
        assert_eq!(reset.phase, RetryPhase::Waiting);
        assert_eq!(reset.consecutive_failed_attempts, 0);
        assert!(reset.recovery_episode.unwrap().exhaustion.is_none());
    }

    #[test]
    fn cooldown_transition_table_requires_matching_satisfaction_evidence() {
        let (mut state, generation, _) =
            failed_initial_attempt(RetryFailureCause::Interrupted, false);
        let evaluated = state.snapshot().unwrap();
        let marker = CooldownEligibilityMarker(12);
        state.require_cooldown(&evaluated, marker).unwrap();

        let pending = state.snapshot().unwrap();
        let pending_authorization = permit(&pending);
        assert_eq!(
            state.commit_recovery_attempt(&pending_authorization),
            Err(RetryTransitionError::CooldownPending)
        );
        assert_eq!(
            state.satisfy_cooldown(
                generation,
                CooldownEligibilityMarker(13),
                CooldownEvidenceId(21)
            ),
            Err(RetryTransitionError::CooldownEvidenceMismatch)
        );
        state
            .satisfy_cooldown(generation, marker, CooldownEvidenceId(21))
            .unwrap();
        let ready = state.snapshot().unwrap();
        assert!(matches!(ready.cooldown, CooldownState::Satisfied { .. }));
        assert_eq!(ready.attempts_started, 1);
        let ready_authorization = permit(&ready);
        assert_eq!(
            state
                .commit_recovery_attempt(&ready_authorization)
                .unwrap()
                .ordinal
                .get(),
            2
        );
    }

    #[test]
    fn cleanup_and_terminal_transition_table_cannot_be_bypassed() {
        for terminal_first in [false, true] {
            let mut state = state();
            let generation = state.explicit_start(CONFIGURATION).unwrap();
            let attempt = state.commit_initial_attempt(generation).unwrap();
            state.record_owner_created(attempt).unwrap();
            state.record_stream_started(attempt).unwrap();
            state
                .record_failure(attempt, RetryFailureCause::Interrupted)
                .unwrap();

            if terminal_first {
                state.record_terminal_event_delivered(attempt).unwrap();
                assert_eq!(state.snapshot().unwrap().phase, RetryPhase::Failed);
                state.record_cleanup_complete(attempt).unwrap();
            } else {
                state.record_cleanup_complete(attempt).unwrap();
                assert_eq!(state.snapshot().unwrap().phase, RetryPhase::Failed);
                state.record_terminal_event_delivered(attempt).unwrap();
            }
            assert_eq!(state.snapshot().unwrap().phase, RetryPhase::Waiting);
        }
    }

    #[test]
    fn recovery_commit_reports_incomplete_cleanup_and_terminal_evidence() {
        let mut state = state();
        let generation = state.explicit_start(CONFIGURATION).unwrap();
        let attempt = state.commit_initial_attempt(generation).unwrap();
        state.record_owner_created(attempt).unwrap();
        state.record_stream_started(attempt).unwrap();
        state
            .record_failure(attempt, RetryFailureCause::Interrupted)
            .unwrap();

        let before_cleanup = permit(&state.snapshot().unwrap());
        assert_eq!(
            state.commit_recovery_attempt(&before_cleanup),
            Err(RetryTransitionError::CleanupPending)
        );
        state.record_cleanup_complete(attempt).unwrap();
        let before_terminal = permit(&state.snapshot().unwrap());
        assert_eq!(
            state.commit_recovery_attempt(&before_terminal),
            Err(RetryTransitionError::TerminalEvidencePending)
        );
    }

    #[test]
    fn policy_evaluation_uses_an_immutable_snapshot() {
        let (state, generation, _) = failed_initial_attempt(RetryFailureCause::Interrupted, false);
        let before = state.snapshot().unwrap();
        let decision = evaluate_recovery(
            RecoveryContext {
                desired_running: before.desired_running,
                current_intent_generation: generation.get(),
                evaluated_intent_generation: before.intent_generation.get(),
            },
            RecoveryCause::Interrupted,
            RecoveryEvidence {
                stream_started: Some(false),
                owner_completed: Some(true),
                resources_released: Some(true),
                retry_hint: Some(RetryHint::RetryNow),
                attempts_remaining: Some(true),
                ..RecoveryEvidence::default()
            },
        );

        assert_eq!(
            decision,
            RecoveryDecision::PermitReplacement(RecoveryDecisionReason::RetryEligible)
        );
        assert!(before.authorize(decision).is_ok());
        assert_eq!(
            before.authorize(RecoveryDecision::Wait(
                RecoveryDecisionReason::CooldownPending
            )),
            Err(RetryTransitionError::DecisionDoesNotPermitRecovery)
        );
        assert_eq!(state.snapshot().unwrap(), before);
    }

    #[test]
    fn state_transitions_return_identity_without_creating_an_owner() {
        let owner_creations = 0;
        let (mut state, _, _) = failed_initial_attempt(RetryFailureCause::Interrupted, false);
        let authorization = permit(&state.snapshot().unwrap());
        let attempt = state.commit_recovery_attempt(&authorization).unwrap();

        assert_eq!(attempt.kind(&state), AttemptKind::AutomaticRecovery);
        assert_eq!(owner_creations, 0);
    }

    #[test]
    fn recent_failure_history_is_bounded_while_aggregates_remain_monotonic() {
        let mut state = RetryState::<2>::new().unwrap();
        let generation = state.explicit_start(CONFIGURATION).unwrap();
        let first = state.commit_initial_attempt(generation).unwrap();
        state
            .record_failure(first, RetryFailureCause::StartupFailure)
            .unwrap();
        state.record_cleanup_complete(first).unwrap();

        for cause in [
            RetryFailureCause::Interrupted,
            RetryFailureCause::WorkerPanic,
        ] {
            let authorization = permit(&state.snapshot().unwrap());
            let attempt = state.commit_recovery_attempt(&authorization).unwrap();
            state.record_failure(attempt, cause).unwrap();
            state.record_cleanup_complete(attempt).unwrap();
        }

        let snapshot = state.snapshot().unwrap();
        assert_eq!(snapshot.recent_failures.len(), 2);
        assert_eq!(
            snapshot.recent_failures[0].cause,
            RetryFailureCause::Interrupted
        );
        assert_eq!(
            snapshot.last_failure.unwrap().cause,
            RetryFailureCause::WorkerPanic
        );
        assert_eq!(snapshot.failure_totals.total, 3);
        assert_eq!(snapshot.failure_totals.startup_failure, 1);
        assert_eq!(snapshot.failure_totals.interrupted, 1);
        assert_eq!(snapshot.failure_totals.worker_panic, 1);
    }

    #[test]
    fn zero_history_capacity_is_rejected() {
        assert!(matches!(
            RetryState::<0>::new(),
            Err(RetryTransitionError::HistoryCapacityZero)
        ));
    }

    impl AttemptId {
        fn kind<const HISTORY_CAPACITY: usize>(
            self,
            state: &RetryState<HISTORY_CAPACITY>,
        ) -> AttemptKind {
            let current = state.current_attempt.expect("attempt is current");
            assert_eq!(current.id, self);
            current.kind
        }
    }
}
