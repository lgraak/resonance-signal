//! Loopback-only HTTP and WebSocket consumer service.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use resonance_api::contract::{ErrorKind, RetryHint, SourceId, StreamEndReason, StreamEvent};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{mpsc, Notify};

use crate::protocol::{
    discovery_json, service_error_json, startup_error_json, EncodedEvent, SessionEncoder,
    PROTOCOL_VERSION,
};
use crate::supervisor::{CaptureSupervisor, CaptureSupervisorStart, CaptureSupervisorWaitError};
use crate::windows::{discover_playback_sources, CaptureOwnerCompletion, PlaybackCaptureIntent};

const DEFAULT_PORT: u16 = 48_480;
const EVENT_QUEUE_CAPACITY: usize = 16;
const MAX_ACTIVE_SESSIONS: usize = 16;
const MAX_SOURCE_ID_BYTES: usize = 256;
const MAX_CLIENT_MESSAGE_BYTES: usize = 1_024;
const SOCKET_WRITE_TIMEOUT: Duration = Duration::from_secs(2);
const CAPTURE_POLL_INTERVAL: Duration = Duration::from_millis(100);
const CAPTURE_STOP_TIMEOUT: Duration = Duration::from_secs(2);
const SERVICE_START_TIMEOUT: Duration = Duration::from_secs(5);
const SERVICE_STOP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentServiceConfig {
    listen: SocketAddr,
}

impl AgentServiceConfig {
    pub fn new(listen: SocketAddr) -> Result<Self, String> {
        if !listen.ip().is_loopback() {
            return Err("Milestone 6U permits loopback listener addresses only".to_string());
        }
        Ok(Self { listen })
    }

    pub fn loopback(port: u16) -> Self {
        Self {
            listen: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
        }
    }

    pub const fn listen(self) -> SocketAddr {
        self.listen
    }
}

impl Default for AgentServiceConfig {
    fn default() -> Self {
        Self::loopback(DEFAULT_PORT)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManagedServiceState {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed(String),
}

pub struct ManagedService {
    state: Arc<Mutex<ManagedServiceState>>,
    shutting_down: Arc<AtomicBool>,
    shutdown_notify: Arc<Notify>,
    stopped_rx: std_mpsc::Receiver<()>,
    worker: Option<thread::JoinHandle<()>>,
}

impl ManagedService {
    pub fn start(config: AgentServiceConfig) -> Result<Self, String> {
        let state = Arc::new(Mutex::new(ManagedServiceState::Starting));
        let shutting_down = Arc::new(AtomicBool::new(false));
        let shutdown_notify = Arc::new(Notify::new());
        let (started_tx, started_rx) = std_mpsc::sync_channel(1);
        let (stopped_tx, stopped_rx) = std_mpsc::sync_channel(1);
        let worker_state = state.clone();
        let worker_shutting_down = shutting_down.clone();
        let worker_notify = shutdown_notify.clone();
        let worker = thread::Builder::new()
            .name("resonance-signal-service".to_string())
            .spawn(move || {
                let result = run_managed(
                    config,
                    worker_state.clone(),
                    worker_shutting_down,
                    worker_notify,
                    started_tx,
                );
                let mut current = worker_state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                if let Err(message) = result {
                    *current = ManagedServiceState::Failed(message);
                } else if !matches!(*current, ManagedServiceState::Failed(_)) {
                    *current = ManagedServiceState::Stopped;
                }
                drop(current);
                let _ = stopped_tx.send(());
            })
            .map_err(|error| format!("failed to start service worker: {error}"))?;

        match started_rx.recv_timeout(SERVICE_START_TIMEOUT) {
            Ok(Ok(())) => Ok(Self {
                state,
                shutting_down,
                shutdown_notify,
                stopped_rx,
                worker: Some(worker),
            }),
            Ok(Err(message)) => {
                let _ = worker.join();
                Err(message)
            }
            Err(error) => {
                shutting_down.store(true, Ordering::Release);
                shutdown_notify.notify_one();
                let _ = worker.join();
                Err(format!(
                    "service startup did not complete within 5 seconds: {error}"
                ))
            }
        }
    }

    pub fn state(&self) -> ManagedServiceState {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn shutdown(&mut self) -> Result<(), String> {
        if self.worker.is_none() {
            return Ok(());
        }
        {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            if matches!(
                *state,
                ManagedServiceState::Running | ManagedServiceState::Starting
            ) {
                *state = ManagedServiceState::Stopping;
            }
        }
        self.shutting_down.store(true, Ordering::Release);
        self.shutdown_notify.notify_one();
        self.stopped_rx
            .recv_timeout(SERVICE_STOP_TIMEOUT)
            .map_err(|error| format!("service did not stop within 5 seconds: {error}"))?;
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| "service worker panicked during shutdown".to_string())?;
        }
        match self.state() {
            ManagedServiceState::Failed(message) => Err(message),
            _ => Ok(()),
        }
    }
}

impl Drop for ManagedService {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[derive(Clone)]
struct AppState {
    active_sessions: Arc<AtomicUsize>,
    discovery_sequence: Arc<AtomicU64>,
    shutting_down: Arc<AtomicBool>,
}

impl AppState {
    fn new() -> Self {
        Self::with_shutdown(Arc::new(AtomicBool::new(false)))
    }

    fn with_shutdown(shutting_down: Arc<AtomicBool>) -> Self {
        Self {
            active_sessions: Arc::new(AtomicUsize::new(0)),
            discovery_sequence: Arc::new(AtomicU64::new(1)),
            shutting_down,
        }
    }
}

pub fn run(config: AgentServiceConfig) -> Result<(), String> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("failed to create service runtime: {error}"))?
        .block_on(serve(config))
}

pub async fn serve(config: AgentServiceConfig) -> Result<(), String> {
    let state = AppState::new();
    let app = router(state.clone());
    let listener = tokio::net::TcpListener::bind(config.listen())
        .await
        .map_err(|error| format!("failed to bind {}: {error}", config.listen()))?;
    let local = listener
        .local_addr()
        .map_err(|error| format!("failed to inspect listener address: {error}"))?;
    if !local.ip().is_loopback() {
        return Err("listener resolved to a non-loopback address".to_string());
    }
    println!("Resonance Signal consumer service listening on http://{local}/v1/");

    let shutdown_state = state.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            if let Err(error) = tokio::signal::ctrl_c().await {
                eprintln!("failed to install shutdown signal handler: {error}");
            }
            shutdown_state.shutting_down.store(true, Ordering::Release);
        })
        .await
        .map_err(|error| format!("consumer service failed: {error}"))
}

fn run_managed(
    config: AgentServiceConfig,
    state: Arc<Mutex<ManagedServiceState>>,
    shutting_down: Arc<AtomicBool>,
    shutdown_notify: Arc<Notify>,
    started_tx: std_mpsc::SyncSender<Result<(), String>>,
) -> Result<(), String> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("failed to create service runtime: {error}"))?
        .block_on(async move {
            let app_state = AppState::with_shutdown(shutting_down.clone());
            let app = router(app_state);
            let listener = match tokio::net::TcpListener::bind(config.listen()).await {
                Ok(listener) => listener,
                Err(error) => {
                    let message = format!("failed to bind {}: {error}", config.listen());
                    let _ = started_tx.send(Err(message.clone()));
                    return Err(message);
                }
            };
            let local = match listener.local_addr() {
                Ok(local) if local.ip().is_loopback() => local,
                Ok(_) => {
                    let message = "listener resolved to a non-loopback address".to_string();
                    let _ = started_tx.send(Err(message.clone()));
                    return Err(message);
                }
                Err(error) => {
                    let message = format!("failed to inspect listener address: {error}");
                    let _ = started_tx.send(Err(message.clone()));
                    return Err(message);
                }
            };
            *state.lock().unwrap_or_else(|error| error.into_inner()) = ManagedServiceState::Running;
            let _ = started_tx.send(Ok(()));
            println!("Resonance Signal consumer service listening on http://{local}/v1/");

            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    shutdown_notify.notified().await;
                    shutting_down.store(true, Ordering::Release);
                })
                .await
                .map_err(|error| format!("consumer service failed: {error}"))
        })
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/status", get(status))
        .route("/v1/sources", get(sources))
        .route("/v1/waveform", get(waveform))
        .fallback(not_found)
        .with_state(state)
}

async fn status(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "protocol_version": PROTOCOL_VERSION,
        "status": if state.shutting_down.load(Ordering::Acquire) { "stopping" } else { "ready" },
        "listener_scope": "loopback",
        "active_stream_sessions": state.active_sessions.load(Ordering::Acquire),
    }))
}

async fn sources(State(state): State<AppState>) -> Response {
    let snapshot = tokio::task::spawn_blocking(discover_playback_sources).await;
    match snapshot {
        Ok(Ok(snapshot)) => {
            let sequence = state.discovery_sequence.fetch_add(1, Ordering::Relaxed);
            Json(discovery_json(
                &snapshot,
                &format!("snapshot-{}-{sequence}", std::process::id()),
            ))
            .into_response()
        }
        Ok(Err(error)) => {
            eprintln!("source discovery failed: {error}");
            discovery_unavailable()
        }
        Err(error) => {
            eprintln!("source discovery worker failed: {error}");
            discovery_unavailable()
        }
    }
}

fn discovery_unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "protocol_version": PROTOCOL_VERSION,
            "error": "source_discovery_unavailable",
        })),
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
struct WaveformQuery {
    source: Option<String>,
    source_id: Option<String>,
}

impl WaveformQuery {
    fn intent(&self) -> Result<PlaybackCaptureIntent, String> {
        match (self.source.as_deref(), self.source_id.as_deref()) {
            (Some("default-playback"), None) => Ok(PlaybackCaptureIntent::DefaultPlayback),
            (None, Some(source_id)) if source_id.len() <= MAX_SOURCE_ID_BYTES => {
                SourceId::new(source_id.to_string())
                    .map(PlaybackCaptureIntent::Explicit)
                    .map_err(|_| "source_id must be a non-empty opaque identity".to_string())
            }
            (None, Some(_)) => Err(format!(
                "source_id must not exceed {MAX_SOURCE_ID_BYTES} UTF-8 bytes"
            )),
            _ => Err(
                "request exactly source=default-playback or one source_id parameter".to_string(),
            ),
        }
    }
}

async fn waveform(
    websocket: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(query): Query<WaveformQuery>,
) -> Response {
    let intent = match query.intent() {
        Ok(intent) => intent,
        Err(message) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "protocol_version": PROTOCOL_VERSION,
                    "error": "invalid_request",
                    "message": message,
                })),
            )
                .into_response();
        }
    };
    let session = match ActiveSession::try_acquire(state.active_sessions.clone()) {
        Some(session) => session,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "protocol_version": PROTOCOL_VERSION,
                    "error": "resource_exhausted",
                })),
            )
                .into_response();
        }
    };
    websocket
        .max_message_size(MAX_CLIENT_MESSAGE_BYTES)
        .max_frame_size(MAX_CLIENT_MESSAGE_BYTES)
        .on_upgrade(move |socket| stream_session(socket, state, intent, session))
}

async fn not_found() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "protocol_version": PROTOCOL_VERSION,
            "error": "not_found",
        })),
    )
}

#[derive(Debug)]
enum SessionMessage {
    Event(EncodedEvent),
    StartupFailure(ErrorKind, RetryHint),
    Finished,
}

async fn stream_session(
    mut socket: WebSocket,
    state: AppState,
    intent: PlaybackCaptureIntent,
    _session: ActiveSession,
) {
    let stop = Arc::new(AtomicBool::new(false));
    let client_stop = Arc::new(AtomicBool::new(false));
    let unhealthy = Arc::new(AtomicBool::new(false));
    let (events_tx, mut events_rx) = mpsc::channel(EVENT_QUEUE_CAPACITY);
    let capture = tokio::task::spawn_blocking({
        let stop = stop.clone();
        let client_stop = client_stop.clone();
        let unhealthy = unhealthy.clone();
        let shutting_down = state.shutting_down.clone();
        move || {
            run_capture_session(
                intent,
                events_tx,
                stop,
                client_stop,
                unhealthy,
                shutting_down,
            )
        }
    });

    loop {
        if unhealthy.load(Ordering::Acquire) {
            let _ = send_json(
                &mut socket,
                service_error_json("consumer_too_slow").to_string(),
            )
            .await;
            break;
        }
        if state.shutting_down.load(Ordering::Acquire) {
            break;
        }

        tokio::select! {
            event = events_rx.recv() => {
                match event {
                    Some(SessionMessage::Event(EncodedEvent::Text(text))) => {
                        if send_json(&mut socket, text).await.is_err() { break; }
                    }
                    Some(SessionMessage::Event(EncodedEvent::Binary(bytes))) => {
                        if send_binary(&mut socket, bytes).await.is_err() { break; }
                    }
                    Some(SessionMessage::StartupFailure(kind, retry)) => {
                        let _ = send_json(&mut socket, startup_error_json(kind, retry).to_string()).await;
                        break;
                    }
                    Some(SessionMessage::Finished) | None => break,
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Ping(bytes))) => {
                        if timed_send(&mut socket, Message::Pong(bytes)).await.is_err() { break; }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(Message::Text(text))) if is_stop_request(&text) => {
                        client_stop.store(true, Ordering::Release);
                        stop.store(true, Ordering::Release);
                    }
                    Some(Ok(Message::Text(_) | Message::Binary(_))) => {
                        let _ = send_json(&mut socket, service_error_json("invalid_request").to_string()).await;
                        break;
                    }
                }
            }
        }
    }
    stop.store(true, Ordering::Release);
    drop(events_rx);
    let _ = capture.await;
    let _ = timed_send(
        &mut socket,
        Message::Close(Some(CloseFrame {
            code: axum::extract::ws::close_code::NORMAL,
            reason: "stream session complete".into(),
        })),
    )
    .await;
}

fn run_capture_session(
    intent: PlaybackCaptureIntent,
    events_tx: mpsc::Sender<SessionMessage>,
    stop: Arc<AtomicBool>,
    client_stop: Arc<AtomicBool>,
    unhealthy: Arc<AtomicBool>,
    shutting_down: Arc<AtomicBool>,
) {
    let callback_tx = events_tx.clone();
    let callback_unhealthy = unhealthy.clone();
    let mut encoder = SessionEncoder::default();
    let mut supervisor = CaptureSupervisor::for_source(intent, move |event: StreamEvent| {
        let event = match event {
            StreamEvent::Ended {
                stream_id,
                reason: StreamEndReason::ProviderShutdown,
            } if client_stop.load(Ordering::Acquire) => StreamEvent::Ended {
                stream_id,
                reason: StreamEndReason::ConsumerCancelled,
            },
            event => event,
        };
        let encoded = match encoder.encode(event) {
            Ok(encoded) => encoded,
            Err(_) => {
                callback_unhealthy.store(true, Ordering::Release);
                return;
            }
        };
        for event in encoded {
            if callback_tx.try_send(SessionMessage::Event(event)).is_err() {
                callback_unhealthy.store(true, Ordering::Release);
                break;
            }
        }
    });

    match supervisor.start() {
        Ok(CaptureSupervisorStart::Started) => {}
        Ok(CaptureSupervisorStart::StoppedBeforeStart) | Err(_) => {
            let _ = events_tx.try_send(SessionMessage::StartupFailure(
                ErrorKind::Internal,
                RetryHint::DoNotRetry,
            ));
            return;
        }
    }

    loop {
        if stop.load(Ordering::Acquire)
            || unhealthy.load(Ordering::Acquire)
            || shutting_down.load(Ordering::Acquire)
        {
            let _ = supervisor.stop(CAPTURE_STOP_TIMEOUT);
            break;
        }
        match supervisor.wait_for_completion(CAPTURE_POLL_INTERVAL) {
            Ok(_) => break,
            Err(CaptureSupervisorWaitError::Timeout(_)) => {}
            Err(CaptureSupervisorWaitError::NotStarted) => break,
        }
    }

    if !supervisor.terminal_observation().started() {
        if let Some(CaptureOwnerCompletion::Failed(error)) = supervisor.owner_completion() {
            let _ = events_tx.try_send(SessionMessage::StartupFailure(
                error.kind(),
                error.retry_hint(),
            ));
        }
    }
    let _ = events_tx.try_send(SessionMessage::Finished);
}

async fn send_json(socket: &mut WebSocket, text: String) -> Result<(), ()> {
    timed_send(socket, Message::Text(text.into())).await
}

async fn send_binary(socket: &mut WebSocket, bytes: Vec<u8>) -> Result<(), ()> {
    timed_send(socket, Message::Binary(bytes.into())).await
}

async fn timed_send(socket: &mut WebSocket, message: Message) -> Result<(), ()> {
    tokio::time::timeout(SOCKET_WRITE_TIMEOUT, socket.send(message))
        .await
        .map_err(|_| ())?
        .map_err(|_| ())
}

fn is_stop_request(text: &str) -> bool {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .is_some_and(|object| {
            object.len() == 1 && object.get("type").and_then(Value::as_str) == Some("stop")
        })
}

struct ActiveSession(Arc<AtomicUsize>);

impl ActiveSession {
    fn try_acquire(active: Arc<AtomicUsize>) -> Option<Self> {
        let mut observed = active.load(Ordering::Acquire);
        loop {
            if observed >= MAX_ACTIVE_SESSIONS {
                return None;
            }
            match active.compare_exchange_weak(
                observed,
                observed + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Some(Self(active)),
                Err(current) => observed = current,
            }
        }
    }
}

impl Drop for ActiveSession {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listener_configuration_rejects_every_non_loopback_address() {
        assert!(AgentServiceConfig::new("127.0.0.1:48480".parse().unwrap()).is_ok());
        assert!(AgentServiceConfig::new("[::1]:48480".parse().unwrap()).is_ok());
        assert!(AgentServiceConfig::new("0.0.0.0:48480".parse().unwrap()).is_err());
        assert!(AgentServiceConfig::new("192.0.2.1:48480".parse().unwrap()).is_err());
        assert!(AgentServiceConfig::new("[::]:48480".parse().unwrap()).is_err());
    }

    #[test]
    fn waveform_selection_is_exact_and_bounded() {
        let default = WaveformQuery {
            source: Some("default-playback".to_string()),
            source_id: None,
        };
        assert_eq!(
            default.intent().unwrap(),
            PlaybackCaptureIntent::DefaultPlayback
        );

        let explicit = WaveformQuery {
            source: None,
            source_id: Some("opaque-a".to_string()),
        };
        assert_eq!(
            explicit.intent().unwrap(),
            PlaybackCaptureIntent::Explicit(SourceId::new("opaque-a").unwrap())
        );
        assert!(WaveformQuery {
            source: Some("default-playback".to_string()),
            source_id: Some("opaque-a".to_string()),
        }
        .intent()
        .is_err());
        assert!(WaveformQuery {
            source: None,
            source_id: Some("x".repeat(MAX_SOURCE_ID_BYTES + 1)),
        }
        .intent()
        .is_err());
    }

    #[test]
    fn event_queue_is_bounded_and_reports_backpressure() {
        let (tx, _rx) = mpsc::channel::<SessionMessage>(1);
        assert!(tx.try_send(SessionMessage::Finished).is_ok());
        assert!(tx.try_send(SessionMessage::Finished).is_err());
    }

    #[test]
    fn concurrent_session_count_is_bounded_and_released() {
        let active = Arc::new(AtomicUsize::new(0));
        let sessions = (0..MAX_ACTIVE_SESSIONS)
            .map(|_| ActiveSession::try_acquire(active.clone()).unwrap())
            .collect::<Vec<_>>();
        assert!(ActiveSession::try_acquire(active.clone()).is_none());
        assert_eq!(active.load(Ordering::Acquire), MAX_ACTIVE_SESSIONS);
        drop(sessions);
        assert_eq!(active.load(Ordering::Acquire), 0);
    }

    #[test]
    fn only_the_exact_bounded_stop_control_is_accepted() {
        assert!(is_stop_request(r#"{"type":"stop"}"#));
        assert!(!is_stop_request(r#"{"type":"stop","extra":true}"#));
        assert!(!is_stop_request(r#"{"type":"start"}"#));
        assert!(!is_stop_request("not-json"));
    }

    #[test]
    fn managed_service_reports_running_and_stops_cleanly() {
        let mut service = ManagedService::start(AgentServiceConfig::loopback(0)).unwrap();
        assert_eq!(service.state(), ManagedServiceState::Running);
        service.shutdown().unwrap();
        assert_eq!(service.state(), ManagedServiceState::Stopped);
    }

    #[test]
    fn managed_service_reports_bind_failure_instead_of_running() {
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = occupied.local_addr().unwrap();
        let error = match ManagedService::start(AgentServiceConfig::new(address).unwrap()) {
            Ok(_) => panic!("managed service unexpectedly bound an occupied address"),
            Err(error) => error,
        };
        assert!(error.contains("failed to bind"));
    }
}
