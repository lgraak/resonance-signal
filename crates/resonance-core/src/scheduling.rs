//! Bounded, provider-independent scheduling of waveform analysis windows.

use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::time::Duration;

use crate::signal::{
    AudioFrame, ChannelLayout, FrameTimestamp, Sample, SampleRate, SignalDataError,
};

const NANOS_PER_SECOND: u128 = 1_000_000_000;
const TIMESTAMP_TOLERANCE_NS: u64 = 1;

/// The default target is one analysis window per 30 FPS visualization update.
pub const DEFAULT_WINDOW_DURATION: Duration = Duration::from_nanos(33_333_333);

/// Configuration for a [`WindowScheduler`].
///
/// Window duration is converted to the nearest whole sample frame for each
/// stream. `max_windows_per_push` bounds work and output allocation caused by a
/// single call; retained partial input is always smaller than one window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowSchedulerConfig {
    target_window_duration: Duration,
    max_windows_per_push: usize,
}

impl WindowSchedulerConfig {
    pub fn new(
        target_window_duration: Duration,
        max_windows_per_push: usize,
    ) -> Result<Self, WindowSchedulerConfigError> {
        if target_window_duration.is_zero() {
            return Err(WindowSchedulerConfigError::ZeroWindowDuration);
        }
        if max_windows_per_push == 0 {
            return Err(WindowSchedulerConfigError::ZeroMaxWindowsPerPush);
        }

        Ok(Self {
            target_window_duration,
            max_windows_per_push,
        })
    }

    pub const fn target_window_duration(self) -> Duration {
        self.target_window_duration
    }

    pub const fn max_windows_per_push(self) -> usize {
        self.max_windows_per_push
    }
}

impl Default for WindowSchedulerConfig {
    fn default() -> Self {
        Self {
            target_window_duration: DEFAULT_WINDOW_DURATION,
            max_windows_per_push: 8,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowSchedulerConfigError {
    ZeroWindowDuration,
    ZeroMaxWindowsPerPush,
}

impl fmt::Display for WindowSchedulerConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroWindowDuration => formatter.write_str("window duration must be non-zero"),
            Self::ZeroMaxWindowsPerPush => {
                formatter.write_str("maximum windows per push must be non-zero")
            }
        }
    }
}

impl Error for WindowSchedulerConfigError {}

/// One complete analysis window associated with its uninterrupted stream.
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduledWindow<StreamIdentity> {
    stream_identity: StreamIdentity,
    frame: AudioFrame,
}

impl<StreamIdentity> ScheduledWindow<StreamIdentity> {
    pub fn stream_identity(&self) -> &StreamIdentity {
        &self.stream_identity
    }

    pub fn frame(&self) -> &AudioFrame {
        &self.frame
    }

    pub fn into_frame(self) -> AudioFrame {
        self.frame
    }
}

/// An observed switch between uninterrupted streams.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamBoundary<StreamIdentity> {
    previous_stream: StreamIdentity,
    current_stream: StreamIdentity,
    discarded_partial_frames: u32,
}

impl<StreamIdentity> StreamBoundary<StreamIdentity> {
    pub fn previous_stream(&self) -> &StreamIdentity {
        &self.previous_stream
    }

    pub fn current_stream(&self) -> &StreamIdentity {
        &self.current_stream
    }

    pub const fn discarded_partial_frames(&self) -> u32 {
        self.discarded_partial_frames
    }
}

/// Complete windows and optional stream-boundary information from one push.
#[derive(Clone, Debug, PartialEq)]
pub struct SchedulingResult<StreamIdentity> {
    windows: Vec<ScheduledWindow<StreamIdentity>>,
    stream_boundary: Option<StreamBoundary<StreamIdentity>>,
}

impl<StreamIdentity> SchedulingResult<StreamIdentity> {
    pub fn windows(&self) -> &[ScheduledWindow<StreamIdentity>] {
        &self.windows
    }

    pub fn into_windows(self) -> Vec<ScheduledWindow<StreamIdentity>> {
        self.windows
    }

    pub fn stream_boundary(&self) -> Option<&StreamBoundary<StreamIdentity>> {
        self.stream_boundary.as_ref()
    }
}

/// Accumulates contiguous waveform frames into bounded, non-overlapping windows.
///
/// The identity type is supplied by the orchestration layer so `resonance-core`
/// does not depend on the consumer API's `StreamId` representation.
#[derive(Clone, Debug)]
pub struct WindowScheduler<StreamIdentity> {
    config: WindowSchedulerConfig,
    active_stream: Option<StreamIdentity>,
    invalid_stream: Option<StreamIdentity>,
    sample_rate: Option<SampleRate>,
    channels: Option<ChannelLayout>,
    timeline_origin: Option<FrameTimestamp>,
    expected_next: Option<FrameTimestamp>,
    buffer_start: Option<FrameTimestamp>,
    samples: VecDeque<Sample>,
}

impl<StreamIdentity: Clone + Eq> WindowScheduler<StreamIdentity> {
    pub fn new(config: WindowSchedulerConfig) -> Self {
        Self {
            config,
            active_stream: None,
            invalid_stream: None,
            sample_rate: None,
            channels: None,
            timeline_origin: None,
            expected_next: None,
            buffer_start: None,
            samples: VecDeque::new(),
        }
    }

    pub const fn config(&self) -> WindowSchedulerConfig {
        self.config
    }

    /// Returns the number of incomplete sample frames currently retained.
    pub fn buffered_frame_count(&self) -> u32 {
        match self.channels.as_ref() {
            Some(channels) => {
                let channel_count = usize::from(channels.channel_count().get());
                u32::try_from(self.samples.len() / channel_count)
                    .expect("retained frames are bounded by a u32 window")
            }
            None => 0,
        }
    }

    /// Pushes one bounded waveform batch.
    pub fn push(
        &mut self,
        stream_identity: StreamIdentity,
        frame: AudioFrame,
    ) -> Result<SchedulingResult<StreamIdentity>, SchedulingError> {
        self.push_batch(stream_identity, [frame])
    }

    /// Pushes zero or more contiguous waveform batches from one stream.
    ///
    /// Empty input is a no-op. A format or continuity error invalidates the
    /// current identity and drops its partial window; that identity cannot be
    /// used again because an interruption must begin a new stream.
    pub fn push_batch(
        &mut self,
        stream_identity: StreamIdentity,
        frames: impl IntoIterator<Item = AudioFrame>,
    ) -> Result<SchedulingResult<StreamIdentity>, SchedulingError> {
        let frames = frames.into_iter().collect::<Vec<_>>();
        if frames.is_empty() {
            return Ok(SchedulingResult {
                windows: Vec::new(),
                stream_boundary: None,
            });
        }

        if self.invalid_stream.as_ref() == Some(&stream_identity) {
            return Err(SchedulingError::StreamIdentityInvalidated);
        }

        let previous_identity = self.active_stream.as_ref();
        let stream_changed = previous_identity.is_some_and(|active| active != &stream_identity);
        let first = &frames[0];
        let sample_rate = first.sample_rate();
        let channels = first.channels().clone();
        let window_frame_count =
            window_frame_count(self.config.target_window_duration, sample_rate)?;
        if !stream_changed
            && self.active_stream.is_some()
            && (self.sample_rate != Some(sample_rate) || self.channels.as_ref() != Some(&channels))
        {
            self.invalidate(stream_identity);
            return Err(SchedulingError::FormatChangedWithinStream);
        }

        let input_frame_count = frames.iter().try_fold(0_u64, |total, frame| {
            total
                .checked_add(u64::from(frame.window().frame_count()))
                .ok_or(SchedulingError::InputFrameCountOverflow)
        })?;
        let maximum_input_frames = u64::from(window_frame_count)
            .checked_mul(self.config.max_windows_per_push as u64)
            .ok_or(SchedulingError::InputFrameCountOverflow)?;
        if input_frame_count > maximum_input_frames {
            return Err(SchedulingError::OversizedInput {
                actual_frames: input_frame_count,
                maximum_frames: maximum_input_frames,
            });
        }

        let mut expected = if stream_changed {
            None
        } else {
            self.expected_next
        };
        for frame in &frames {
            if frame.sample_rate() != sample_rate || frame.channels() != &channels {
                self.invalidate(stream_identity.clone());
                return Err(SchedulingError::FormatChangedWithinStream);
            }
            if let Some(expected_start) = expected {
                let actual_start = frame.window().start();
                if actual_start.frame_index() != expected_start.frame_index() {
                    self.invalidate(stream_identity.clone());
                    return Err(SchedulingError::FrameIndexDiscontinuity {
                        expected: expected_start.frame_index(),
                        actual: actual_start.frame_index(),
                    });
                }
                if actual_start
                    .stream_time_ns()
                    .abs_diff(expected_start.stream_time_ns())
                    > TIMESTAMP_TOLERANCE_NS
                {
                    self.invalidate(stream_identity.clone());
                    return Err(SchedulingError::TimestampDiscontinuity {
                        expected_ns: expected_start.stream_time_ns(),
                        actual_ns: actual_start.stream_time_ns(),
                    });
                }
            }
            expected = match advance_timestamp(
                frame.window().start(),
                frame.window().frame_count(),
                sample_rate,
            ) {
                Ok(expected) => Some(expected),
                Err(error) => {
                    self.invalidate(stream_identity.clone());
                    return Err(error);
                }
            };
        }

        let discarded_partial_frames = self.buffered_frame_count();
        let stream_boundary = if stream_changed {
            Some(StreamBoundary {
                previous_stream: self
                    .active_stream
                    .as_ref()
                    .expect("a changed stream has a previous identity")
                    .clone(),
                current_stream: stream_identity.clone(),
                discarded_partial_frames,
            })
        } else {
            None
        };

        if stream_changed || self.active_stream.is_none() {
            self.clear_stream_state();
            self.active_stream = Some(stream_identity.clone());
            self.invalid_stream = None;
            self.sample_rate = Some(sample_rate);
            self.channels = Some(channels.clone());
            self.timeline_origin = Some(first.window().start());
        }

        let channel_count = usize::from(channels.channel_count().get());
        let samples_per_window = window_frame_count as usize * channel_count;
        let mut windows = Vec::with_capacity(
            usize::try_from(input_frame_count / u64::from(window_frame_count))
                .unwrap_or(self.config.max_windows_per_push),
        );

        for frame in frames {
            if self.samples.is_empty() {
                self.buffer_start = Some(frame.window().start());
            }
            self.samples.extend(frame.into_samples());

            while self.samples.len() >= samples_per_window {
                let start = self
                    .buffer_start
                    .expect("a non-empty scheduler buffer has a start timestamp");
                let window_samples = self.samples.drain(..samples_per_window).collect();
                let output = AudioFrame::new(start, sample_rate, channels.clone(), window_samples)
                    .map_err(SchedulingError::InvalidSignalData)?;
                windows.push(ScheduledWindow {
                    stream_identity: stream_identity.clone(),
                    frame: output,
                });

                if self.samples.is_empty() {
                    self.buffer_start = None;
                } else {
                    let next_frame_index = start
                        .frame_index()
                        .checked_add(u64::from(window_frame_count))
                        .ok_or(SchedulingError::TimestampOverflow)?;
                    self.buffer_start = Some(timestamp_for_frame_index(
                        self.timeline_origin
                            .expect("an active stream has a timeline origin"),
                        next_frame_index,
                        sample_rate,
                    )?);
                }
            }
        }

        self.expected_next = expected;

        Ok(SchedulingResult {
            windows,
            stream_boundary,
        })
    }

    fn invalidate(&mut self, stream_identity: StreamIdentity) {
        self.clear_stream_state();
        self.active_stream = Some(stream_identity.clone());
        self.invalid_stream = Some(stream_identity);
    }

    fn clear_stream_state(&mut self) {
        self.sample_rate = None;
        self.channels = None;
        self.timeline_origin = None;
        self.expected_next = None;
        self.buffer_start = None;
        self.samples.clear();
    }
}

impl<StreamIdentity: Clone + Eq> Default for WindowScheduler<StreamIdentity> {
    fn default() -> Self {
        Self::new(WindowSchedulerConfig::default())
    }
}

fn window_frame_count(duration: Duration, sample_rate: SampleRate) -> Result<u32, SchedulingError> {
    let duration_ns = duration.as_nanos();
    let numerator = duration_ns
        .checked_mul(u128::from(sample_rate.hz()))
        .ok_or(SchedulingError::WindowFrameCountOutOfRange)?;
    let rounded = numerator
        .checked_add(NANOS_PER_SECOND / 2)
        .ok_or(SchedulingError::WindowFrameCountOutOfRange)?
        / NANOS_PER_SECOND;
    if rounded == 0 || rounded > u128::from(u32::MAX) {
        return Err(SchedulingError::WindowFrameCountOutOfRange);
    }

    Ok(rounded as u32)
}

fn advance_timestamp(
    start: FrameTimestamp,
    frame_count: u32,
    sample_rate: SampleRate,
) -> Result<FrameTimestamp, SchedulingError> {
    let frame_index = start
        .frame_index()
        .checked_add(u64::from(frame_count))
        .ok_or(SchedulingError::TimestampOverflow)?;
    let time_offset_ns = u64::from(frame_count)
        .checked_mul(1_000_000_000)
        .ok_or(SchedulingError::TimestampOverflow)?
        / u64::from(sample_rate.hz());
    let stream_time_ns = start
        .stream_time_ns()
        .checked_add(time_offset_ns)
        .ok_or(SchedulingError::TimestampOverflow)?;
    Ok(FrameTimestamp::new(frame_index, stream_time_ns))
}

fn timestamp_for_frame_index(
    origin: FrameTimestamp,
    frame_index: u64,
    sample_rate: SampleRate,
) -> Result<FrameTimestamp, SchedulingError> {
    let elapsed_frames = frame_index
        .checked_sub(origin.frame_index())
        .ok_or(SchedulingError::TimestampOverflow)?;
    let time_offset_ns = elapsed_frames
        .checked_mul(1_000_000_000)
        .ok_or(SchedulingError::TimestampOverflow)?
        / u64::from(sample_rate.hz());
    let stream_time_ns = origin
        .stream_time_ns()
        .checked_add(time_offset_ns)
        .ok_or(SchedulingError::TimestampOverflow)?;
    Ok(FrameTimestamp::new(frame_index, stream_time_ns))
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum SchedulingError {
    WindowFrameCountOutOfRange,
    InputFrameCountOverflow,
    OversizedInput {
        actual_frames: u64,
        maximum_frames: u64,
    },
    StreamIdentityInvalidated,
    FormatChangedWithinStream,
    FrameIndexDiscontinuity {
        expected: u64,
        actual: u64,
    },
    TimestampDiscontinuity {
        expected_ns: u64,
        actual_ns: u64,
    },
    TimestampOverflow,
    InvalidSignalData(SignalDataError),
}

impl fmt::Display for SchedulingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WindowFrameCountOutOfRange => formatter.write_str(
                "target window duration does not produce a valid u32 sample-frame count",
            ),
            Self::InputFrameCountOverflow => formatter.write_str("input frame count overflowed"),
            Self::OversizedInput {
                actual_frames,
                maximum_frames,
            } => write!(
                formatter,
                "input contains {actual_frames} sample frames; maximum is {maximum_frames}"
            ),
            Self::StreamIdentityInvalidated => formatter.write_str(
                "stream identity was invalidated by a discontinuity and cannot be reused",
            ),
            Self::FormatChangedWithinStream => {
                formatter.write_str("sample rate or channel layout changed within a stream")
            }
            Self::FrameIndexDiscontinuity { expected, actual } => write!(
                formatter,
                "expected source frame index {expected}; got {actual}"
            ),
            Self::TimestampDiscontinuity {
                expected_ns,
                actual_ns,
            } => write!(
                formatter,
                "expected stream timestamp {expected_ns} ns; got {actual_ns} ns"
            ),
            Self::TimestampOverflow => formatter.write_str("stream timestamp overflowed"),
            Self::InvalidSignalData(error) => error.fmt(formatter),
        }
    }
}

impl Error for SchedulingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSignalData(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processing::{calculate_levels, WaveformWindow};
    use crate::signal::{ChannelPosition, SignalDataError};

    fn config() -> WindowSchedulerConfig {
        WindowSchedulerConfig::new(Duration::from_millis(10), 2).unwrap()
    }

    fn mono_frame(start: u64, frames: u32) -> Result<AudioFrame, SignalDataError> {
        AudioFrame::new(
            FrameTimestamp::new(start, start * 1_000_000),
            SampleRate::new(1_000).unwrap(),
            ChannelLayout::positioned([ChannelPosition::Mono]).unwrap(),
            (0..frames)
                .map(|offset| (start + u64::from(offset)) as f32)
                .collect(),
        )
    }

    #[test]
    fn exact_window_completion_feeds_existing_processing() {
        let mut scheduler = WindowScheduler::new(config());
        let result = scheduler
            .push("stream-a", mono_frame(0, 10).unwrap())
            .unwrap();

        assert_eq!(result.windows().len(), 1);
        assert_eq!(result.windows()[0].frame().window().frame_count(), 10);
        assert_eq!(scheduler.buffered_frame_count(), 0);
        let levels = calculate_levels(WaveformWindow::entire(result.windows()[0].frame())).unwrap();
        assert_eq!(levels.window(), result.windows()[0].frame().window());
    }

    #[test]
    fn partial_frames_accumulate_without_early_output() {
        let mut scheduler = WindowScheduler::new(config());

        let first = scheduler
            .push("stream-a", mono_frame(0, 4).unwrap())
            .unwrap();
        assert!(first.windows().is_empty());
        assert_eq!(scheduler.buffered_frame_count(), 4);

        let second = scheduler
            .push("stream-a", mono_frame(4, 6).unwrap())
            .unwrap();
        assert_eq!(second.windows().len(), 1);
        assert_eq!(second.windows()[0].frame().samples().len(), 10);
        assert_eq!(scheduler.buffered_frame_count(), 0);
    }

    #[test]
    fn multiple_input_frames_create_multiple_windows() {
        let mut scheduler = WindowScheduler::new(config());
        let result = scheduler
            .push_batch(
                "stream-a",
                [mono_frame(0, 10).unwrap(), mono_frame(10, 10).unwrap()],
            )
            .unwrap();

        assert_eq!(result.windows().len(), 2);
        assert_eq!(
            result.windows()[0].frame().window().start().frame_index(),
            0
        );
        assert_eq!(
            result.windows()[1].frame().window().start().frame_index(),
            10
        );
    }

    #[test]
    fn oversized_input_is_rejected_without_changing_buffer() {
        let mut scheduler = WindowScheduler::new(config());
        let error = scheduler
            .push("stream-a", mono_frame(0, 21).unwrap())
            .unwrap_err();

        assert_eq!(
            error,
            SchedulingError::OversizedInput {
                actual_frames: 21,
                maximum_frames: 20,
            }
        );
        assert_eq!(scheduler.buffered_frame_count(), 0);
    }

    #[test]
    fn frame_index_gap_invalidates_stream_and_drops_partial_window() {
        let mut scheduler = WindowScheduler::new(config());
        scheduler
            .push("stream-a", mono_frame(0, 4).unwrap())
            .unwrap();

        assert_eq!(
            scheduler.push("stream-a", mono_frame(5, 5).unwrap()),
            Err(SchedulingError::FrameIndexDiscontinuity {
                expected: 4,
                actual: 5,
            })
        );
        assert_eq!(scheduler.buffered_frame_count(), 0);
        assert_eq!(
            scheduler.push("stream-a", mono_frame(5, 5).unwrap()),
            Err(SchedulingError::StreamIdentityInvalidated)
        );
    }

    #[test]
    fn timestamp_gap_is_rejected_even_when_frame_indices_are_contiguous() {
        let mut scheduler = WindowScheduler::new(config());
        scheduler
            .push("stream-a", mono_frame(0, 4).unwrap())
            .unwrap();
        let late = AudioFrame::new(
            FrameTimestamp::new(4, 5_000_000),
            SampleRate::new(1_000).unwrap(),
            ChannelLayout::positioned([ChannelPosition::Mono]).unwrap(),
            vec![0.0; 6],
        )
        .unwrap();

        assert_eq!(
            scheduler.push("stream-a", late),
            Err(SchedulingError::TimestampDiscontinuity {
                expected_ns: 4_000_000,
                actual_ns: 5_000_000,
            })
        );
    }

    #[test]
    fn format_change_invalidates_the_existing_stream_identity() {
        let mut scheduler = WindowScheduler::new(config());
        scheduler
            .push("stream-a", mono_frame(0, 4).unwrap())
            .unwrap();
        let changed_rate = AudioFrame::new(
            FrameTimestamp::new(4, 4_000_000),
            SampleRate::new(2_000).unwrap(),
            ChannelLayout::positioned([ChannelPosition::Mono]).unwrap(),
            vec![0.0; 6],
        )
        .unwrap();

        assert_eq!(
            scheduler.push("stream-a", changed_rate),
            Err(SchedulingError::FormatChangedWithinStream)
        );
        assert_eq!(scheduler.buffered_frame_count(), 0);
    }

    #[test]
    fn stream_boundary_reports_discarded_partial_frames() {
        let mut scheduler = WindowScheduler::new(config());
        scheduler
            .push("stream-a", mono_frame(0, 4).unwrap())
            .unwrap();

        let result = scheduler
            .push("stream-b", mono_frame(0, 10).unwrap())
            .unwrap();
        let boundary = result.stream_boundary().unwrap();

        assert_eq!(boundary.previous_stream(), &"stream-a");
        assert_eq!(boundary.current_stream(), &"stream-b");
        assert_eq!(boundary.discarded_partial_frames(), 4);
        assert_eq!(result.windows()[0].stream_identity(), &"stream-b");
    }

    #[test]
    fn empty_input_is_a_no_op() {
        let mut scheduler = WindowScheduler::new(config());
        let result = scheduler
            .push_batch("stream-a", Vec::<AudioFrame>::new())
            .unwrap();

        assert!(result.windows().is_empty());
        assert!(result.stream_boundary().is_none());
        assert_eq!(scheduler.buffered_frame_count(), 0);
    }

    #[test]
    fn thirty_and_sixty_fps_durations_round_to_exact_frame_counts() {
        let channels = ChannelLayout::positioned([ChannelPosition::Mono]).unwrap();
        let thirty_fps_frame = AudioFrame::new(
            FrameTimestamp::new(0, 0),
            SampleRate::new(48_000).unwrap(),
            channels.clone(),
            vec![0.0; 1_600],
        )
        .unwrap();
        let mut thirty_fps = WindowScheduler::default();
        let result = thirty_fps.push("stream-a", thirty_fps_frame).unwrap();
        assert_eq!(result.windows()[0].frame().window().frame_count(), 1_600);

        let config = WindowSchedulerConfig::new(Duration::from_nanos(16_666_667), 1).unwrap();
        let mut sixty_fps = WindowScheduler::new(config);
        let sixty_fps_frame = AudioFrame::new(
            FrameTimestamp::new(0, 0),
            SampleRate::new(48_000).unwrap(),
            channels,
            vec![0.0; 800],
        )
        .unwrap();
        let result = sixty_fps.push("stream-a", sixty_fps_frame).unwrap();
        assert_eq!(result.windows()[0].frame().window().frame_count(), 800);
    }
}
