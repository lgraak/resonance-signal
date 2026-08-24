//! Recovery-disabled capture lifecycle supervision.

use std::error::Error;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use resonance_api::contract::{ErrorKind, RetryHint, StreamEndReason, StreamEvent};

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
    fn create(&mut self, on_event: CaptureEventCallback) -> Box<dyn SupervisedCaptureOwner>;
}

/// Production factory for the Windows WASAPI capture owner.
#[derive(Clone, Copy, Debug, Default)]
pub struct WasapiCaptureOwnerFactory;

impl CaptureOwnerFactory for WasapiCaptureOwnerFactory {
    fn create(&mut self, on_event: CaptureEventCallback) -> Box<dyn SupervisedCaptureOwner> {
        Box::new(CaptureOwner::new(on_event))
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

/// Result of the one owner creation attempt permitted in this milestone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureSupervisorStart {
    Started,
    StoppedBeforeStart,
}

/// Why the supervisor could not start an owner.
#[derive(Debug)]
pub enum CaptureSupervisorStartError {
    NotIdle(CaptureSupervisorState),
    Owner(CaptureOwnerStartError),
}

impl fmt::Display for CaptureSupervisorStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotIdle(state) => {
                write!(formatter, "capture supervisor cannot start from {state:?}")
            }
            Self::Owner(error) => error.fmt(formatter),
        }
    }
}

impl Error for CaptureSupervisorStartError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
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
    error: Option<(ErrorKind, RetryHint)>,
    end_reason: Option<StreamEndReason>,
}

impl CaptureTerminalObservation {
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

/// Owns capture intent and exactly one single-use owner.
///
/// This initial supervisor deliberately contains no recovery policy. It records
/// whether a future replacement could be considered, but never creates one.
pub struct CaptureSupervisor {
    factory: Box<dyn CaptureOwnerFactory>,
    on_event: Option<CaptureEventCallback>,
    observation: Arc<Mutex<CaptureTerminalObservation>>,
    owner: Option<Box<dyn SupervisedCaptureOwner>>,
    state: CaptureSupervisorState,
    desired_running: bool,
    resources_released: bool,
    completion: Option<CaptureSupervisorCompletion>,
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
        Self {
            factory: Box::new(factory),
            on_event: Some(Box::new(on_event)),
            observation: Arc::new(Mutex::new(CaptureTerminalObservation::default())),
            owner: None,
            state: CaptureSupervisorState::Idle,
            desired_running: false,
            resources_released: false,
            completion: None,
        }
    }

    pub const fn state(&self) -> CaptureSupervisorState {
        self.state
    }

    pub const fn desired_running(&self) -> bool {
        self.desired_running
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

    /// Reports the mechanical boundary required by future recovery policy.
    ///
    /// Eligibility is not permission and does not create another owner. Future
    /// policy must additionally inspect the typed outcome and configured intent.
    pub fn replacement_eligible(&self) -> bool {
        self.desired_running
            && self.terminal_observation().terminal_event_delivered()
            && self.completion.is_some()
            && self.resources_released
    }

    /// Creates and starts the only owner this recovery-disabled supervisor may
    /// ever create.
    pub fn start(&mut self) -> Result<CaptureSupervisorStart, CaptureSupervisorStartError> {
        if self.state != CaptureSupervisorState::Idle {
            return Err(CaptureSupervisorStartError::NotIdle(self.state));
        }

        self.desired_running = true;
        let observation = self.observation.clone();
        let mut consumer = self
            .on_event
            .take()
            .expect("an idle supervisor retains its event callback");
        let on_event: CaptureEventCallback = Box::new(move |event| {
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
            if let Some(error) = error {
                observed.error = Some(error);
            }
            if let Some(end_reason) = end_reason {
                observed.end_reason = Some(end_reason);
            }
        });

        let mut owner = self.factory.create(on_event);
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
        self.desired_running = false;
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
            CaptureSupervisorState::Completed => {}
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
            self.completion = Some(CaptureSupervisorCompletion::from(completion));
            self.resources_released = true;
            self.state = CaptureSupervisorState::Completed;
        }
        Ok(self
            .completion
            .as_ref()
            .expect("a completed supervisor retains its outcome"))
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

    use resonance_api::contract::{ProviderError, StreamId};

    use crate::windows::{CaptureReport, CaptureRunError};

    struct FakePlan {
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
        fn create(&mut self, on_event: CaptureEventCallback) -> Box<dyn SupervisedCaptureOwner> {
            self.creations.fetch_add(1, Ordering::SeqCst);
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum_active.fetch_max(active, Ordering::SeqCst);
            Box::new(FakeOwner {
                plan: self.plan.take(),
                on_event,
                active: self.active.clone(),
                stop_requests: self.stop_requests.clone(),
                resources_released: self.resources_released.clone(),
                completion: None,
            })
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
            start_error: false,
            timeout_waits: 0,
            events: vec![ended(StreamEndReason::ConsumerCancelled)],
            completion: CaptureOwnerCompletion::Finished(fake_report(CaptureEnd::DurationElapsed)),
        }
    }

    fn stopped_plan() -> FakePlan {
        FakePlan {
            start_error: false,
            timeout_waits: 0,
            events: vec![ended(StreamEndReason::ProviderShutdown)],
            completion: CaptureOwnerCompletion::Finished(fake_report(CaptureEnd::StopRequested)),
        }
    }

    fn ended(reason: StreamEndReason) -> StreamEvent {
        StreamEvent::Ended {
            stream_id: StreamId::new("fake-stream").unwrap(),
            reason,
        }
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
            start_error: false,
            timeout_waits: 0,
            events: vec![
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
}
