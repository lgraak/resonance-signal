//! Capture lifecycle supervision with advisory-only recovery evaluation.

use std::error::Error;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use resonance_api::contract::{ErrorKind, RetryHint, StreamEndReason, StreamEvent};

use crate::recovery::{
    evaluate_recovery, DeviceUnavailableCause, RecoveryCause, RecoveryContext, RecoveryDecision,
    RecoveryEvidence,
};
use crate::recovery_config::{
    RecoveryConfigurationInput, RecoveryConfigurationSnapshot, RecoveryConfigurationVersion,
};
use crate::retry_state::{AttemptId, CooldownState, RetryFailureCause, RetrySnapshot, RetryState};
use crate::windows::{
    CaptureEnd, CaptureOwner, CaptureOwnerCompletion, CaptureOwnerShutdownTimeout,
    CaptureOwnerStart, CaptureOwnerStartError,
};

/// Consumer callback passed through the supervisor to one capture owner.
pub type CaptureEventCallback = Box<dyn FnMut(StreamEvent) + Send + 'static>;

/// The narrow owner surface required by [`CaptureSupervisor`].
///
/// A successful completion wait must mean that the owner worker has joined and
/// all nested capture resources and callbacks have been released. This trait is
/// an injection seam for lifecycle tests, not a platform-neutral capture API.
pub trait SupervisedCaptureOwner {
    fn start(&mut self) -> Result<CaptureOwnerStart, CaptureOwnerStartError>;
    fn request_stop(&self);
    fn wait_for_completion(
        &mut self,
        timeout: Duration,
    ) -> Result<&CaptureOwnerCompletion, CaptureOwnerShutdownTimeout>;
    fn completion(&self) -> Option<&CaptureOwnerCompletion>;
}

impl SupervisedCaptureOwner for CaptureOwner {
    fn start(&mut self) -> Result<CaptureOwnerStart, CaptureOwnerStartError> {
        CaptureOwner::start(self)
    }

    fn request_stop(&self) {
        CaptureOwner::request_stop(self);
    }

    fn wait_for_completion(
        &mut self,
        timeout: Duration,
    ) -> Result<&CaptureOwnerCompletion, CaptureOwnerShutdownTimeout> {
        CaptureOwner::wait_for_completion(self, timeout)
    }

    fn completion(&self) -> Option<&CaptureOwnerCompletion> {
        CaptureOwner::completion(self)
    }
}

/// Creates one single-use capture owner for a supervised capture intent.
pub trait CaptureOwnerFactory {
    fn create(
        &mut self,
        on_event: CaptureEventCallback,
    ) -> Result<Box<dyn SupervisedCaptureOwner>, CaptureOwnerConstructionError>;
}

/// Failure before a capture owner exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureOwnerConstructionError {
    message: String,
}

impl CaptureOwnerConstructionError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CaptureOwnerConstructionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for CaptureOwnerConstructionError {}

/// Production factory for the Windows WASAPI capture owner.
#[derive(Clone, Copy, Debug, Default)]
pub struct WasapiCaptureOwnerFactory;

impl CaptureOwnerFactory for WasapiCaptureOwnerFactory {
    fn create(
        &mut self,
        on_event: CaptureEventCallback,
    ) -> Result<Box<dyn SupervisedCaptureOwner>, CaptureOwnerConstructionError> {
        Ok(Box::new(CaptureOwner::new(on_event)))
    }
}

/// Stable supervisor lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureSupervisorState {
    Idle,
    Running,
    Stopping,
    Completed,
}

/// Result of the one owner creation attempt permitted for this supervisor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureSupervisorStart {
    Started,
    StoppedBeforeStart,
}

/// Why the supervisor could not start an owner.
#[derive(Debug)]
pub enum CaptureSupervisorStartError {
    NotIdle(CaptureSupervisorState),
    Construction(CaptureOwnerConstructionError),
    Owner(CaptureOwnerStartError),
}

impl fmt::Display for CaptureSupervisorStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotIdle(state) => {
                write!(formatter, "capture supervisor cannot start from {state:?}")
            }
            Self::Construction(error) => error.fmt(formatter),
            Self::Owner(error) => error.fmt(formatter),
        }
    }
}

impl Error for CaptureSupervisorStartError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Construction(error) => Some(error),
            Self::Owner(error) => Some(error),
            Self::NotIdle(_) => None,
        }
    }
}

/// Failure while observing owner completion without requesting stop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureSupervisorWaitError {
    NotStarted,
    Timeout(CaptureOwnerShutdownTimeout),
}

impl fmt::Display for CaptureSupervisorWaitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotStarted => formatter.write_str("capture supervisor has not been started"),
            Self::Timeout(error) => error.fmt(formatter),
        }
    }
}

impl Error for CaptureSupervisorWaitError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Timeout(error) => Some(error),
            Self::NotStarted => None,
        }
    }
}

/// Machine-actionable owner outcome retained after joined completion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureSupervisorCompletion {
    StoppedBeforeStart,
    ConstructionFailed,
    Finished(CaptureEnd),
    Failed {
        kind: ErrorKind,
        retry_hint: RetryHint,
    },
    StartFailed,
    Panicked,
}

impl From<&CaptureOwnerCompletion> for CaptureSupervisorCompletion {
    fn from(completion: &CaptureOwnerCompletion) -> Self {
        match completion {
            CaptureOwnerCompletion::StoppedBeforeStart => Self::StoppedBeforeStart,
            CaptureOwnerCompletion::Finished(report) => Self::Finished(report.end),
            CaptureOwnerCompletion::Failed(error) => Self::Failed {
                kind: error.kind(),
                retry_hint: error.retry_hint(),
            },
            CaptureOwnerCompletion::StartFailed(_) => Self::StartFailed,
            CaptureOwnerCompletion::Panicked => Self::Panicked,
        }
    }
}

/// Typed terminal lifecycle information delivered to the consumer callback.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureTerminalObservation {
    started: bool,
    error: Option<(ErrorKind, RetryHint)>,
    end_reason: Option<StreamEndReason>,
}

impl CaptureTerminalObservation {
    pub const fn started(self) -> bool {
        self.started
    }

    pub const fn error(self) -> Option<(ErrorKind, RetryHint)> {
        self.error
    }

    pub const fn end_reason(self) -> Option<StreamEndReason> {
        self.end_reason
    }

    pub const fn terminal_event_delivered(self) -> bool {
        self.end_reason.is_some()
    }
}

const RETRY_HISTORY_CAPACITY: usize = 16;
const RECOVERY_DISABLED_CONFIGURATION_VERSION: RecoveryConfigurationVersion =
    RecoveryConfigurationVersion::new(1).expect("disabled configuration version is nonzero");

/// Immutable agent-internal lifecycle evidence supplied to recovery policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RecoveryLifecycleSnapshot {
    pub(crate) attempt_id: AttemptId,
    pub(crate) stream_started: bool,
    pub(crate) terminal_event_delivered: bool,
    pub(crate) owner_completed: bool,
    pub(crate) resources_released: bool,
    pub(crate) completion: CaptureSupervisorCompletion,
}

/// Owned, immutable input for one side-effect-free recovery evaluation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RecoveryEvaluationSnapshot {
    pub(crate) configuration: RecoveryConfigurationSnapshot,
    pub(crate) retry_state: RetrySnapshot,
    pub(crate) lifecycle: RecoveryLifecycleSnapshot,
    pub(crate) cause: RecoveryCause,
    pub(crate) policy_evidence: RecoveryEvidence,
}

impl RecoveryEvaluationSnapshot {
    fn configuration_is_current(&self, current: &RecoveryConfigurationSnapshot) -> bool {
        self.configuration.is_same_configuration(current)
            && self.retry_state.configuration_id == current.identity()
    }

    fn evaluate(
        &self,
        current_desired_running: bool,
        current_intent_generation: u64,
    ) -> RecoveryDecision {
        evaluate_recovery(
            RecoveryContext {
                desired_running: current_desired_running,
                current_intent_generation,
                evaluated_intent_generation: self.retry_state.intent_generation.get(),
            },
            self.cause,
            self.policy_evidence,
        )
    }
}

/// Advisory policy result retained for inspection and stale-state checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RecordedRecoveryEvaluation {
    pub(crate) snapshot: RecoveryEvaluationSnapshot,
    pub(crate) decision: RecoveryDecision,
}

/// Owns capture intent, retry state, and exactly one single-use owner.
///
/// Recovery policy results are recorded as advisory data. No result can create
/// an owner, start a retry, schedule work, or wait for time or hardware.
pub struct CaptureSupervisor {
    factory: Box<dyn CaptureOwnerFactory>,
    on_event: Option<CaptureEventCallback>,
    observation: Arc<Mutex<CaptureTerminalObservation>>,
    owner: Option<Box<dyn SupervisedCaptureOwner>>,
    state: CaptureSupervisorState,
    recovery_configuration: RecoveryConfigurationSnapshot,
    retry_state: RetryState<RETRY_HISTORY_CAPACITY>,
    resources_released: bool,
    completion: Option<CaptureSupervisorCompletion>,
    recovery_evaluation: Option<RecordedRecoveryEvaluation>,
}

impl CaptureSupervisor {
    /// Creates an inert production supervisor without allocating capture
    /// resources or starting an owner.
    pub fn new(on_event: impl FnMut(StreamEvent) + Send + 'static) -> Self {
        Self::with_factory(WasapiCaptureOwnerFactory, on_event)
    }

    /// Creates an inert supervisor with an injected single-use owner factory.
    pub fn with_factory(
        factory: impl CaptureOwnerFactory + 'static,
        on_event: impl FnMut(StreamEvent) + Send + 'static,
    ) -> Self {
        let recovery_configuration =
            RecoveryConfigurationInput::recovery_disabled(RECOVERY_DISABLED_CONFIGURATION_VERSION)
                .validate()
                .expect("the explicit recovery-disabled configuration is valid");
        Self {
            factory: Box::new(factory),
            on_event: Some(Box::new(on_event)),
            observation: Arc::new(Mutex::new(CaptureTerminalObservation::default())),
            owner: None,
            state: CaptureSupervisorState::Idle,
            recovery_configuration,
            retry_state: RetryState::new().expect("retry history capacity is nonzero"),
            resources_released: false,
            completion: None,
            recovery_evaluation: None,
        }
    }

    pub const fn state(&self) -> CaptureSupervisorState {
        self.state
    }

    pub const fn desired_running(&self) -> bool {
        self.retry_state.desired_running()
    }

    pub fn terminal_observation(&self) -> CaptureTerminalObservation {
        *lock_unpoisoned(&self.observation)
    }

    pub const fn completion(&self) -> Option<CaptureSupervisorCompletion> {
        self.completion
    }

    /// Returns the retained full owner outcome after joined completion.
    ///
    /// `None` is expected for a stop before the supervisor created an owner.
    pub fn owner_completion(&self) -> Option<&CaptureOwnerCompletion> {
        self.owner.as_ref().and_then(|owner| owner.completion())
    }

    pub const fn resources_released(&self) -> bool {
        self.resources_released
    }

    #[cfg(test)]
    pub(crate) fn retry_snapshot(&self) -> Option<RetrySnapshot> {
        self.retry_state.snapshot().ok()
    }

    #[cfg(test)]
    pub(crate) fn recovery_evaluation(&self) -> Option<&RecordedRecoveryEvaluation> {
        self.recovery_evaluation.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn recovery_evaluation_is_current(
        &self,
        evaluation: &RecordedRecoveryEvaluation,
    ) -> bool {
        evaluation
            .snapshot
            .configuration_is_current(&self.recovery_configuration)
            && self
                .retry_state
                .is_current_snapshot(&evaluation.snapshot.retry_state)
    }

    /// Reports the mechanical boundary required by future recovery policy.
    ///
    /// Eligibility is not permission and does not create another owner. Future
    /// policy must additionally inspect the typed outcome and configured intent.
    pub fn replacement_eligible(&self) -> bool {
        self.desired_running()
            && self.terminal_observation().terminal_event_delivered()
            && self.completion.is_some()
            && self.resources_released
    }

    /// Commits one attempt, then creates and starts this supervisor's only owner.
    pub fn start(&mut self) -> Result<CaptureSupervisorStart, CaptureSupervisorStartError> {
        if self.state != CaptureSupervisorState::Idle {
            return Err(CaptureSupervisorStartError::NotIdle(self.state));
        }

        let generation = self
            .retry_state
            .explicit_start(self.recovery_configuration.identity())
            .expect("an idle supervisor can begin a new capture intent");
        let attempt_id = self
            .retry_state
            .commit_initial_attempt(generation)
            .expect("the first factory call commits exactly one attempt");
        let observation = self.observation.clone();
        let mut consumer = self
            .on_event
            .take()
            .expect("an idle supervisor retains its event callback");
        let on_event: CaptureEventCallback = Box::new(move |event| {
            let started = matches!(&event, StreamEvent::Started(_));
            let error = match &event {
                StreamEvent::Error(error) => Some((error.kind(), error.retry_hint())),
                _ => None,
            };
            let end_reason = match &event {
                StreamEvent::Ended { reason, .. } => Some(*reason),
                _ => None,
            };

            consumer(event);

            let mut observed = lock_unpoisoned(&observation);
            if started {
                observed.started = true;
            }
            if let Some(error) = error {
                observed.error = Some(error);
            }
            if let Some(end_reason) = end_reason {
                observed.end_reason = Some(end_reason);
            }
        });

        let mut owner = match self.factory.create(on_event) {
            Ok(owner) => owner,
            Err(error) => {
                self.retry_state
                    .record_failure(attempt_id, RetryFailureCause::OwnerConstructionFailure)
                    .expect("construction failure belongs to the committed attempt");
                self.retry_state
                    .record_cleanup_complete(attempt_id)
                    .expect("failed construction leaves no owner resources");
                self.resources_released = true;
                self.completion = Some(CaptureSupervisorCompletion::ConstructionFailed);
                self.state = CaptureSupervisorState::Completed;
                self.record_recovery_evaluation();
                return Err(CaptureSupervisorStartError::Construction(error));
            }
        };
        self.retry_state
            .record_owner_created(attempt_id)
            .expect("factory success belongs to the committed attempt");
        let start = owner.start();
        self.owner = Some(owner);
        match start {
            Ok(CaptureOwnerStart::Started) => {
                self.state = CaptureSupervisorState::Running;
                Ok(CaptureSupervisorStart::Started)
            }
            Ok(CaptureOwnerStart::StopAlreadyRequested) => {
                self.record_owner_completion(Duration::ZERO)
                    .expect("a pre-stopped owner has immediate completion");
                Ok(CaptureSupervisorStart::StoppedBeforeStart)
            }
            Err(error) => {
                self.record_owner_completion(Duration::ZERO)
                    .expect("an owner start failure has immediate completion");
                Err(CaptureSupervisorStartError::Owner(error))
            }
        }
    }

    /// Observes natural owner completion without changing running intent.
    pub fn wait_for_completion(
        &mut self,
        timeout: Duration,
    ) -> Result<&CaptureSupervisorCompletion, CaptureSupervisorWaitError> {
        if self.state == CaptureSupervisorState::Idle {
            return Err(CaptureSupervisorWaitError::NotStarted);
        }
        self.record_owner_completion(timeout)
            .map_err(CaptureSupervisorWaitError::Timeout)
    }

    /// Disables future recovery intent, requests owner stop, and waits for
    /// joined completion. Repeated calls are harmless and retain the outcome.
    pub fn stop(
        &mut self,
        timeout: Duration,
    ) -> Result<&CaptureSupervisorCompletion, CaptureOwnerShutdownTimeout> {
        self.disable_recovery_intent();
        match self.state {
            CaptureSupervisorState::Idle => {
                self.resources_released = true;
                self.completion = Some(CaptureSupervisorCompletion::StoppedBeforeStart);
                self.state = CaptureSupervisorState::Completed;
            }
            CaptureSupervisorState::Running | CaptureSupervisorState::Stopping => {
                self.state = CaptureSupervisorState::Stopping;
                self.owner
                    .as_ref()
                    .expect("a running supervisor retains its owner")
                    .request_stop();
                return self.record_owner_completion(timeout);
            }
            CaptureSupervisorState::Completed => {
                self.record_recovery_evaluation();
            }
        }
        Ok(self
            .completion
            .as_ref()
            .expect("a completed supervisor retains its outcome"))
    }

    fn record_owner_completion(
        &mut self,
        timeout: Duration,
    ) -> Result<&CaptureSupervisorCompletion, CaptureOwnerShutdownTimeout> {
        if self.completion.is_none() {
            let completion = self
                .owner
                .as_mut()
                .expect("a started supervisor retains its owner")
                .wait_for_completion(timeout)?;
            let completion = CaptureSupervisorCompletion::from(completion);
            self.completion = Some(completion);
            self.resources_released = true;
            self.state = CaptureSupervisorState::Completed;
            self.record_attempt_completion(completion);
            self.record_recovery_evaluation();
        }
        Ok(self
            .completion
            .as_ref()
            .expect("a completed supervisor retains its outcome"))
    }

    fn disable_recovery_intent(&mut self) {
        let Some(generation) = self.retry_state.intent_generation() else {
            return;
        };
        if self.retry_state.desired_running() {
            self.retry_state
                .explicit_stop(generation)
                .expect("the supervisor stops only its current intent");
        }
    }

    fn record_attempt_completion(&mut self, completion: CaptureSupervisorCompletion) {
        let observation = self.terminal_observation();
        let attempt_id = self
            .retry_state
            .snapshot()
            .expect("a started supervisor has retry state")
            .current_attempt
            .expect("a started supervisor has one committed attempt")
            .id;

        if observation.started() {
            let attempt = self
                .retry_state
                .snapshot()
                .expect("a started supervisor has retry state")
                .current_attempt
                .expect("a started supervisor has one committed attempt");
            if !attempt.stream_started {
                self.retry_state
                    .record_stream_started(attempt_id)
                    .expect("Started belongs to the current attempt");
            }
        }

        if let Some(cause) = retry_failure_for(completion) {
            self.retry_state
                .record_failure(attempt_id, cause)
                .expect("one terminal failure belongs to the current attempt");
        } else {
            self.retry_state
                .record_normal_completion(attempt_id)
                .expect("one normal completion belongs to the current attempt");
        }

        if observation.started() && observation.terminal_event_delivered() {
            self.retry_state
                .record_terminal_event_delivered(attempt_id)
                .expect("terminal delivery belongs to the current started attempt");
        }
        self.retry_state
            .record_cleanup_complete(attempt_id)
            .expect("joined completion releases the current attempt");
    }

    fn recovery_snapshot(&self) -> Option<RecoveryEvaluationSnapshot> {
        let retry_state = self.retry_state.snapshot().ok()?;
        let attempt = retry_state.current_attempt?;
        let completion = self.completion?;
        let observation = self.terminal_observation();
        let attempts_remaining = retry_state
            .recovery_episode
            .is_none_or(|episode| episode.exhaustion.is_none());
        let cooldown_complete = matches!(
            retry_state.cooldown,
            CooldownState::NotRequired | CooldownState::Satisfied { .. }
        );
        let retry_hint = match completion {
            CaptureSupervisorCompletion::Failed { retry_hint, .. } => Some(retry_hint),
            _ => observation.error().map(|(_, retry_hint)| retry_hint),
        };
        let cause = if !retry_state.desired_running {
            RecoveryCause::ExplicitStop
        } else if let Some(failure) = retry_state.last_failure {
            failure.cause.into()
        } else {
            RecoveryCause::NormalShutdown
        };
        let lifecycle = RecoveryLifecycleSnapshot {
            attempt_id: attempt.id,
            stream_started: attempt.stream_started,
            terminal_event_delivered: attempt.terminal_event_delivered,
            owner_completed: self.completion.is_some(),
            resources_released: attempt.resources_released,
            completion,
        };
        let policy_evidence = RecoveryEvidence {
            stream_started: Some(lifecycle.stream_started),
            terminal_event_delivered: Some(lifecycle.terminal_event_delivered),
            owner_completed: Some(lifecycle.owner_completed),
            resources_released: Some(lifecycle.resources_released),
            retry_hint,
            attempts_remaining: Some(attempts_remaining),
            cooldown_complete: Some(cooldown_complete),
            ..RecoveryEvidence::default()
        };
        Some(RecoveryEvaluationSnapshot {
            configuration: self.recovery_configuration.clone(),
            retry_state,
            lifecycle,
            cause,
            policy_evidence,
        })
    }

    fn record_recovery_evaluation(&mut self) {
        let Some(snapshot) = self.recovery_snapshot() else {
            return;
        };
        debug_assert!(snapshot.configuration_is_current(&self.recovery_configuration));
        let decision = snapshot.evaluate(
            self.retry_state.desired_running(),
            self.retry_state
                .intent_generation()
                .expect("an evaluation snapshot has an intent generation")
                .get(),
        );
        self.recovery_evaluation = Some(RecordedRecoveryEvaluation { snapshot, decision });
    }
}

fn retry_failure_for(completion: CaptureSupervisorCompletion) -> Option<RetryFailureCause> {
    match completion {
        CaptureSupervisorCompletion::ConstructionFailed => {
            Some(RetryFailureCause::OwnerConstructionFailure)
        }
        CaptureSupervisorCompletion::StartFailed => Some(RetryFailureCause::StartupFailure),
        CaptureSupervisorCompletion::Panicked => Some(RetryFailureCause::WorkerPanic),
        CaptureSupervisorCompletion::Failed { kind, .. } => Some(match kind {
            ErrorKind::SourceUnavailable => {
                RetryFailureCause::DeviceUnavailable(DeviceUnavailableCause::Invalidated)
            }
            ErrorKind::StreamInterrupted => RetryFailureCause::Interrupted,
            ErrorKind::UnsupportedFormat => RetryFailureCause::UnsupportedFormat,
            ErrorKind::ResourceExhausted => RetryFailureCause::ResourceExhausted,
            _ => RetryFailureCause::InternalFailure,
        }),
        CaptureSupervisorCompletion::StoppedBeforeStart
        | CaptureSupervisorCompletion::Finished(_) => None,
    }
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use resonance_api::contract::{
        ChannelLayout, ChannelPosition, ProviderError, SampleRate, SourceId, SourceKind,
        StreamDescriptor, StreamId,
    };

    use crate::capture::CaptureError;
    use crate::recovery::RecoveryDecisionReason;
    use crate::retry_state::{AttemptLifecycle, RetryPhase};
    use crate::windows::{CaptureReport, CaptureRunError};

    struct FakePlan {
        construction_error: bool,
        start_error: bool,
        timeout_waits: usize,
        events: Vec<StreamEvent>,
        completion: CaptureOwnerCompletion,
    }

    struct FakeFactory {
        plan: Option<FakePlan>,
        creations: Arc<AtomicUsize>,
        active: Arc<AtomicUsize>,
        maximum_active: Arc<AtomicUsize>,
        stop_requests: Arc<AtomicUsize>,
        resources_released: Arc<AtomicUsize>,
    }

    struct FakeOwner {
        plan: Option<FakePlan>,
        on_event: CaptureEventCallback,
        active: Arc<AtomicUsize>,
        stop_requests: Arc<AtomicUsize>,
        resources_released: Arc<AtomicUsize>,
        completion: Option<CaptureOwnerCompletion>,
    }

    impl CaptureOwnerFactory for FakeFactory {
        fn create(
            &mut self,
            on_event: CaptureEventCallback,
        ) -> Result<Box<dyn SupervisedCaptureOwner>, CaptureOwnerConstructionError> {
            self.creations.fetch_add(1, Ordering::SeqCst);
            if self.plan.as_ref().unwrap().construction_error {
                self.plan.take();
                return Err(CaptureOwnerConstructionError::new(
                    "fake construction failure",
                ));
            }
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum_active.fetch_max(active, Ordering::SeqCst);
            Ok(Box::new(FakeOwner {
                plan: self.plan.take(),
                on_event,
                active: self.active.clone(),
                stop_requests: self.stop_requests.clone(),
                resources_released: self.resources_released.clone(),
                completion: None,
            }))
        }
    }

    impl SupervisedCaptureOwner for FakeOwner {
        fn start(&mut self) -> Result<CaptureOwnerStart, CaptureOwnerStartError> {
            if self.plan.as_ref().unwrap().start_error {
                self.completion = Some(CaptureOwnerCompletion::StartFailed(
                    "fake start failure".to_string(),
                ));
                Err(CaptureOwnerStartError::Spawn(io::Error::other(
                    "fake start failure",
                )))
            } else {
                Ok(CaptureOwnerStart::Started)
            }
        }

        fn request_stop(&self) {
            self.stop_requests.fetch_add(1, Ordering::SeqCst);
        }

        fn wait_for_completion(
            &mut self,
            timeout: Duration,
        ) -> Result<&CaptureOwnerCompletion, CaptureOwnerShutdownTimeout> {
            if self.completion.is_none() {
                let plan = self.plan.as_mut().unwrap();
                if plan.timeout_waits > 0 {
                    plan.timeout_waits -= 1;
                    return Err(CaptureOwnerShutdownTimeout::new(timeout));
                }
                let mut plan = self.plan.take().unwrap();
                for event in plan.events.drain(..) {
                    (self.on_event)(event);
                }
                self.active.fetch_sub(1, Ordering::SeqCst);
                self.resources_released.fetch_add(1, Ordering::SeqCst);
                self.completion = Some(plan.completion);
            } else if self.active.load(Ordering::SeqCst) > 0 {
                self.active.fetch_sub(1, Ordering::SeqCst);
                self.resources_released.fetch_add(1, Ordering::SeqCst);
            }
            Ok(self.completion.as_ref().unwrap())
        }

        fn completion(&self) -> Option<&CaptureOwnerCompletion> {
            self.completion.as_ref()
        }
    }

    struct Harness {
        supervisor: CaptureSupervisor,
        creations: Arc<AtomicUsize>,
        active: Arc<AtomicUsize>,
        maximum_active: Arc<AtomicUsize>,
        stop_requests: Arc<AtomicUsize>,
        resources_released: Arc<AtomicUsize>,
    }

    fn harness(plan: FakePlan, on_event: impl FnMut(StreamEvent) + Send + 'static) -> Harness {
        let creations = Arc::new(AtomicUsize::new(0));
        let active = Arc::new(AtomicUsize::new(0));
        let maximum_active = Arc::new(AtomicUsize::new(0));
        let stop_requests = Arc::new(AtomicUsize::new(0));
        let resources_released = Arc::new(AtomicUsize::new(0));
        let factory = FakeFactory {
            plan: Some(plan),
            creations: creations.clone(),
            active: active.clone(),
            maximum_active: maximum_active.clone(),
            stop_requests: stop_requests.clone(),
            resources_released: resources_released.clone(),
        };
        Harness {
            supervisor: CaptureSupervisor::with_factory(factory, on_event),
            creations,
            active,
            maximum_active,
            stop_requests,
            resources_released,
        }
    }

    fn normal_plan() -> FakePlan {
        FakePlan {
            construction_error: false,
            start_error: false,
            timeout_waits: 0,
            events: vec![started(), ended(StreamEndReason::ConsumerCancelled)],
            completion: CaptureOwnerCompletion::Finished(fake_report(CaptureEnd::DurationElapsed)),
        }
    }

    fn stopped_plan() -> FakePlan {
        FakePlan {
            construction_error: false,
            start_error: false,
            timeout_waits: 0,
            events: vec![started(), ended(StreamEndReason::ProviderShutdown)],
            completion: CaptureOwnerCompletion::Finished(fake_report(CaptureEnd::StopRequested)),
        }
    }

    fn interrupted_plan() -> FakePlan {
        FakePlan {
            construction_error: false,
            start_error: false,
            timeout_waits: 0,
            events: vec![
                started(),
                StreamEvent::Error(ProviderError::new(
                    ErrorKind::StreamInterrupted,
                    resonance_api::contract::ErrorScope::Subscription,
                    RetryHint::RetryNow,
                    "fake interruption",
                )),
                ended(StreamEndReason::Failed),
            ],
            completion: CaptureOwnerCompletion::Failed(CaptureRunError::Capture(
                CaptureError::DataDiscontinuity,
            )),
        }
    }

    fn ended(reason: StreamEndReason) -> StreamEvent {
        StreamEvent::Ended {
            stream_id: StreamId::new("fake-stream").unwrap(),
            reason,
        }
    }

    fn started() -> StreamEvent {
        StreamEvent::Started(StreamDescriptor::new(
            StreamId::new("fake-stream").unwrap(),
            SourceId::new("fake-source").unwrap(),
            SourceKind::Playback,
            SampleRate::new(48_000).unwrap(),
            ChannelLayout::positioned([ChannelPosition::FrontLeft, ChannelPosition::FrontRight])
                .unwrap(),
        ))
    }

    fn fake_report(end: CaptureEnd) -> CaptureReport {
        CaptureReport {
            endpoint_name: "fake endpoint".to_string(),
            native_sample_rate_hz: 48_000,
            native_channel_count: 2,
            output_sample_rate_hz: 48_000,
            output_channel_count: 2,
            maximum_packet_frames: 480,
            packet_count: 0,
            audio_frame_count: 0,
            source_frame_count: 0,
            minimum_packet_frames: None,
            maximum_observed_packet_frames: None,
            minimum_callback_interval: None,
            maximum_callback_interval: None,
            maximum_callback_duration: None,
            minimum_qpc_delta: None,
            maximum_qpc_delta: None,
            initial_discontinuity_observed: false,
            end,
            end_diagnostic: None,
        }
    }

    #[test]
    fn start_creates_and_tracks_exactly_one_active_owner() {
        let mut harness = harness(normal_plan(), |_| {});

        assert_eq!(
            harness.supervisor.start().unwrap(),
            CaptureSupervisorStart::Started
        );
        assert_eq!(harness.supervisor.state(), CaptureSupervisorState::Running);
        assert_eq!(harness.creations.load(Ordering::SeqCst), 1);
        assert_eq!(harness.active.load(Ordering::SeqCst), 1);
        assert_eq!(harness.maximum_active.load(Ordering::SeqCst), 1);
        assert!(!harness.supervisor.replacement_eligible());
        assert!(matches!(
            harness.supervisor.start(),
            Err(CaptureSupervisorStartError::NotIdle(
                CaptureSupervisorState::Running
            ))
        ));
        assert_eq!(harness.creations.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn stop_before_start_creates_no_owner_and_repeated_stop_is_stable() {
        let mut harness = harness(normal_plan(), |_| {});

        assert_eq!(
            *harness.supervisor.stop(Duration::ZERO).unwrap(),
            CaptureSupervisorCompletion::StoppedBeforeStart
        );
        assert_eq!(
            *harness.supervisor.stop(Duration::ZERO).unwrap(),
            CaptureSupervisorCompletion::StoppedBeforeStart
        );
        assert_eq!(harness.creations.load(Ordering::SeqCst), 0);
        assert_eq!(
            harness.supervisor.state(),
            CaptureSupervisorState::Completed
        );
        assert!(!harness.supervisor.desired_running());
        assert!(!harness.supervisor.replacement_eligible());
    }

    #[test]
    fn stop_running_owner_disables_intent_waits_for_release_and_is_idempotent() {
        let mut harness = harness(stopped_plan(), |_| {});
        harness.supervisor.start().unwrap();

        assert_eq!(
            *harness.supervisor.stop(Duration::ZERO).unwrap(),
            CaptureSupervisorCompletion::Finished(CaptureEnd::StopRequested)
        );
        assert_eq!(harness.resources_released.load(Ordering::SeqCst), 1);
        assert_eq!(harness.active.load(Ordering::SeqCst), 0);
        assert!(!harness.supervisor.desired_running());
        assert!(!harness.supervisor.replacement_eligible());
        assert_eq!(
            *harness.supervisor.stop(Duration::ZERO).unwrap(),
            CaptureSupervisorCompletion::Finished(CaptureEnd::StopRequested)
        );
        assert_eq!(harness.stop_requests.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn stop_timeout_retains_stopping_state_and_owner_for_repeated_wait() {
        let mut plan = stopped_plan();
        plan.timeout_waits = 1;
        let mut harness = harness(plan, |_| {});
        harness.supervisor.start().unwrap();

        let timeout = harness
            .supervisor
            .stop(Duration::from_millis(5))
            .unwrap_err();
        assert_eq!(timeout.timeout(), Duration::from_millis(5));
        assert_eq!(harness.supervisor.state(), CaptureSupervisorState::Stopping);
        assert!(!harness.supervisor.desired_running());
        assert!(!harness.supervisor.resources_released());
        assert_eq!(harness.active.load(Ordering::SeqCst), 1);

        assert_eq!(
            *harness.supervisor.stop(Duration::ZERO).unwrap(),
            CaptureSupervisorCompletion::Finished(CaptureEnd::StopRequested)
        );
        assert_eq!(harness.stop_requests.load(Ordering::SeqCst), 2);
        assert_eq!(harness.resources_released.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn normal_completion_records_terminal_delivery_before_replacement_eligibility() {
        let delivered = Arc::new(AtomicUsize::new(0));
        let callback_delivered = delivered.clone();
        let mut harness = harness(normal_plan(), move |event| {
            if matches!(event, StreamEvent::Ended { .. }) {
                callback_delivered.fetch_add(1, Ordering::SeqCst);
            }
        });
        harness.supervisor.start().unwrap();

        assert!(!harness.supervisor.replacement_eligible());
        assert_eq!(delivered.load(Ordering::SeqCst), 0);
        assert_eq!(
            *harness
                .supervisor
                .wait_for_completion(Duration::ZERO)
                .unwrap(),
            CaptureSupervisorCompletion::Finished(CaptureEnd::DurationElapsed)
        );
        assert_eq!(delivered.load(Ordering::SeqCst), 1);
        assert_eq!(harness.resources_released.load(Ordering::SeqCst), 1);
        assert_eq!(
            harness.supervisor.terminal_observation().end_reason(),
            Some(StreamEndReason::ConsumerCancelled)
        );
        assert!(harness.supervisor.replacement_eligible());
        assert!(matches!(
            harness.supervisor.start(),
            Err(CaptureSupervisorStartError::NotIdle(
                CaptureSupervisorState::Completed
            ))
        ));
        assert_eq!(harness.creations.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn capture_failure_retains_typed_event_and_completion_outcomes() {
        let error = ProviderError::new(
            ErrorKind::SourceUnavailable,
            resonance_api::contract::ErrorScope::Subscription,
            RetryHint::WaitForSource,
            "fake failure",
        );
        let plan = FakePlan {
            construction_error: false,
            start_error: false,
            timeout_waits: 0,
            events: vec![
                started(),
                StreamEvent::Error(error),
                ended(StreamEndReason::SourceEnded),
            ],
            completion: CaptureOwnerCompletion::Failed(CaptureRunError::SourceUnavailable(
                "fake failure".to_string(),
            )),
        };
        let mut harness = harness(plan, |_| {});
        harness.supervisor.start().unwrap();

        assert_eq!(
            *harness
                .supervisor
                .wait_for_completion(Duration::ZERO)
                .unwrap(),
            CaptureSupervisorCompletion::Failed {
                kind: ErrorKind::SourceUnavailable,
                retry_hint: RetryHint::WaitForSource,
            }
        );
        assert_eq!(
            harness.supervisor.terminal_observation().error(),
            Some((ErrorKind::SourceUnavailable, RetryHint::WaitForSource))
        );
        assert!(harness.supervisor.replacement_eligible());
    }

    #[test]
    fn startup_failure_and_panic_without_terminal_event_are_not_replacement_eligible() {
        let start_plan = FakePlan {
            construction_error: false,
            start_error: true,
            timeout_waits: 0,
            events: vec![],
            completion: CaptureOwnerCompletion::StartFailed("fake".to_string()),
        };
        let mut start_harness = harness(start_plan, |_| {});

        assert!(matches!(
            start_harness.supervisor.start(),
            Err(CaptureSupervisorStartError::Owner(
                CaptureOwnerStartError::Spawn(_)
            ))
        ));
        assert_eq!(
            start_harness.supervisor.completion(),
            Some(CaptureSupervisorCompletion::StartFailed)
        );
        assert!(start_harness.supervisor.resources_released());
        assert!(!start_harness.supervisor.replacement_eligible());

        let panic_plan = FakePlan {
            construction_error: false,
            start_error: false,
            timeout_waits: 0,
            events: vec![],
            completion: CaptureOwnerCompletion::Panicked,
        };
        let mut panic_harness = harness(panic_plan, |_| {});
        panic_harness.supervisor.start().unwrap();
        assert_eq!(
            *panic_harness
                .supervisor
                .wait_for_completion(Duration::ZERO)
                .unwrap(),
            CaptureSupervisorCompletion::Panicked
        );
        assert!(!panic_harness.supervisor.replacement_eligible());
    }

    #[test]
    fn retry_state_is_supervisor_owned_and_one_attempt_retains_all_lifecycle_facts() {
        let mut harness = harness(normal_plan(), |_| {});

        harness.supervisor.start().unwrap();
        let committed = harness.supervisor.retry_snapshot().unwrap();
        let attempt_id = committed.current_attempt.unwrap().id;
        assert!(committed.desired_running);
        assert_eq!(committed.attempts_started, 1);
        assert_eq!(attempt_id.intent_generation, committed.intent_generation);
        assert_eq!(attempt_id.ordinal.get(), 1);
        assert_eq!(
            committed.current_attempt.unwrap().lifecycle,
            AttemptLifecycle::OwnerCreated
        );

        harness
            .supervisor
            .wait_for_completion(Duration::ZERO)
            .unwrap();

        let completed = harness.supervisor.retry_snapshot().unwrap();
        let attempt = completed.current_attempt.unwrap();
        assert_eq!(completed.attempts_started, 1);
        assert_eq!(attempt.id, attempt_id);
        assert!(attempt.stream_started);
        assert!(attempt.terminal_event_delivered);
        assert!(attempt.resources_released);
        assert_eq!(attempt.lifecycle, AttemptLifecycle::CleanupComplete);
        assert_eq!(attempt.failure, None);
        assert!(completed.state_revision > committed.state_revision);
    }

    #[test]
    fn construction_and_startup_failures_each_count_the_committed_attempt_once() {
        let construction_plan = FakePlan {
            construction_error: true,
            start_error: false,
            timeout_waits: 0,
            events: vec![],
            completion: CaptureOwnerCompletion::StoppedBeforeStart,
        };
        let mut construction = harness(construction_plan, |_| {});
        assert!(matches!(
            construction.supervisor.start(),
            Err(CaptureSupervisorStartError::Construction(_))
        ));
        let construction_state = construction.supervisor.retry_snapshot().unwrap();
        let construction_attempt = construction_state.current_attempt.unwrap();
        assert_eq!(construction_state.attempts_started, 1);
        assert_eq!(
            construction_attempt.failure,
            Some(RetryFailureCause::OwnerConstructionFailure)
        );
        assert!(construction_attempt.resources_released);
        assert_eq!(construction.creations.load(Ordering::SeqCst), 1);
        assert_eq!(construction.active.load(Ordering::SeqCst), 0);

        let startup_plan = FakePlan {
            construction_error: false,
            start_error: true,
            timeout_waits: 0,
            events: vec![],
            completion: CaptureOwnerCompletion::StartFailed("fake".to_string()),
        };
        let mut startup = harness(startup_plan, |_| {});
        assert!(matches!(
            startup.supervisor.start(),
            Err(CaptureSupervisorStartError::Owner(_))
        ));
        let startup_state = startup.supervisor.retry_snapshot().unwrap();
        assert_eq!(startup_state.attempts_started, 1);
        assert_eq!(
            startup_state.current_attempt.unwrap().failure,
            Some(RetryFailureCause::StartupFailure)
        );
        assert_eq!(startup.creations.load(Ordering::SeqCst), 1);
        assert_eq!(startup.resources_released.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn immutable_snapshot_supplies_complete_interruption_evidence_and_policy_is_advisory() {
        let mut harness = harness(interrupted_plan(), |_| {});
        harness.supervisor.start().unwrap();
        harness
            .supervisor
            .wait_for_completion(Duration::ZERO)
            .unwrap();

        let before = harness.supervisor.retry_snapshot().unwrap();
        let first = harness.supervisor.recovery_snapshot().unwrap();
        let second = harness.supervisor.recovery_snapshot().unwrap();
        let after = harness.supervisor.retry_snapshot().unwrap();
        assert_eq!(first, second);
        assert_eq!(before, after);
        assert_eq!(
            first.retry_state.configuration_id,
            first.configuration.identity()
        );
        assert_eq!(
            first.configuration.identity().version(),
            RECOVERY_DISABLED_CONFIGURATION_VERSION
        );
        assert_eq!(
            first.lifecycle.attempt_id,
            before.current_attempt.unwrap().id
        );
        assert_eq!(first.policy_evidence.stream_started, Some(true));
        assert_eq!(first.policy_evidence.terminal_event_delivered, Some(true));
        assert_eq!(first.policy_evidence.owner_completed, Some(true));
        assert_eq!(first.policy_evidence.resources_released, Some(true));
        assert_eq!(first.policy_evidence.retry_hint, Some(RetryHint::RetryNow));
        assert_eq!(first.policy_evidence.attempts_remaining, Some(true));

        let evaluation = harness.supervisor.recovery_evaluation().unwrap();
        assert_eq!(
            evaluation.decision,
            RecoveryDecision::PermitReplacement(RecoveryDecisionReason::RetryEligible)
        );
        assert!(harness
            .supervisor
            .recovery_evaluation_is_current(evaluation));
        assert_eq!(harness.creations.load(Ordering::SeqCst), 1);
        assert_eq!(harness.maximum_active.load(Ordering::SeqCst), 1);
        assert_eq!(harness.active.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn validating_configuration_cannot_mutate_supervisor_or_create_an_owner() {
        let harness = harness(interrupted_plan(), |_| {});
        let before_state = harness.supervisor.state();
        let before_snapshot = harness.supervisor.retry_snapshot();

        let version = RecoveryConfigurationVersion::new(2).expect("test version is nonzero");
        let accepted = RecoveryConfigurationInput::recovery_disabled(version)
            .validate()
            .expect("explicit disabled configuration is valid");

        assert_eq!(accepted.identity().version(), version);
        assert_eq!(harness.supervisor.state(), before_state);
        assert_eq!(harness.supervisor.retry_snapshot(), before_snapshot);
        assert_eq!(harness.creations.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn explicit_stop_invalidates_prior_evaluation_and_late_completion_cannot_recover() {
        let mut completed = harness(interrupted_plan(), |_| {});
        completed.supervisor.start().unwrap();
        completed
            .supervisor
            .wait_for_completion(Duration::ZERO)
            .unwrap();
        let old_evaluation = completed.supervisor.recovery_evaluation().unwrap().clone();
        assert_eq!(
            old_evaluation.decision,
            RecoveryDecision::PermitReplacement(RecoveryDecisionReason::RetryEligible)
        );

        completed.supervisor.stop(Duration::ZERO).unwrap();

        assert!(!completed
            .supervisor
            .recovery_evaluation_is_current(&old_evaluation));
        let stopped = completed.supervisor.retry_snapshot().unwrap();
        assert!(!stopped.desired_running);
        assert_eq!(stopped.phase, RetryPhase::Stopped);
        assert_eq!(
            completed.supervisor.recovery_evaluation().unwrap().decision,
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::ExplicitStop)
        );
        assert_eq!(completed.creations.load(Ordering::SeqCst), 1);

        let mut late = harness(interrupted_plan(), |_| {});
        late.supervisor.start().unwrap();
        late.supervisor.stop(Duration::ZERO).unwrap();
        let late_state = late.supervisor.retry_snapshot().unwrap();
        assert!(!late_state.desired_running);
        assert_eq!(late_state.attempts_started, 1);
        assert_eq!(late_state.phase, RetryPhase::Stopped);
        assert_eq!(
            late.supervisor.recovery_evaluation().unwrap().decision,
            RecoveryDecision::RemainStopped(RecoveryDecisionReason::ExplicitStop)
        );
        assert_eq!(late.creations.load(Ordering::SeqCst), 1);
        assert_eq!(late.maximum_active.load(Ordering::SeqCst), 1);
    }
}
