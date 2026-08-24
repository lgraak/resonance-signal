//! Provider-independent audio signal data structures.

use std::error::Error;
use std::fmt;

/// A normalized, linear PCM sample.
///
/// `-1.0` and `1.0` represent nominal negative and positive full scale. Values
/// outside that range are permitted so processing does not silently clip
/// headroom, but all samples must be finite.
pub type Sample = f32;

/// The number of sample frames produced per second.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SampleRate(u32);

impl SampleRate {
    /// Creates a non-zero sample rate.
    pub fn new(hz: u32) -> Result<Self, SignalFormatError> {
        if hz == 0 {
            return Err(SignalFormatError::ZeroSampleRate);
        }

        Ok(Self(hz))
    }

    /// Returns the sample rate in hertz.
    pub const fn hz(self) -> u32 {
        self.0
    }
}

/// A non-zero number of channels.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ChannelCount(u16);

impl ChannelCount {
    /// Creates a non-zero channel count.
    pub fn new(count: u16) -> Result<Self, SignalFormatError> {
        if count == 0 {
            return Err(SignalFormatError::ZeroChannels);
        }

        Ok(Self(count))
    }

    /// Returns the number of channels.
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// A semantic speaker or microphone position.
///
/// Sources whose positions cannot be mapped without guessing use a discrete
/// [`ChannelLayout`] instead.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ChannelPosition {
    Mono,
    FrontLeft,
    FrontRight,
    FrontCenter,
    LowFrequency,
    BackLeft,
    BackRight,
    FrontLeftOfCenter,
    FrontRightOfCenter,
    BackCenter,
    SideLeft,
    SideRight,
    TopCenter,
    TopFrontLeft,
    TopFrontCenter,
    TopFrontRight,
    TopBackLeft,
    TopBackCenter,
    TopBackRight,
}

/// The order and meaning of channels in a signal frame.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChannelLayout {
    /// Channels are ordered but have no portable semantic positions.
    Discrete(ChannelCount),
    /// Channels are ordered by the entries in this vector.
    Positioned(Vec<ChannelPosition>),
}

impl ChannelLayout {
    /// Creates a layout with a known count but unknown semantic positions.
    pub fn discrete(count: ChannelCount) -> Self {
        Self::Discrete(count)
    }

    /// Creates an ordered, positioned channel layout.
    pub fn positioned(
        positions: impl Into<Vec<ChannelPosition>>,
    ) -> Result<Self, SignalFormatError> {
        let positions = positions.into();
        let count = u16::try_from(positions.len())
            .map_err(|_| SignalFormatError::TooManyChannelPositions(positions.len()))?;
        ChannelCount::new(count)?;

        if positions.len() > 1 && positions.contains(&ChannelPosition::Mono) {
            return Err(SignalFormatError::MonoWithOtherChannels);
        }

        for (index, position) in positions.iter().enumerate() {
            if positions[..index].contains(position) {
                return Err(SignalFormatError::DuplicateChannelPosition(*position));
            }
        }

        Ok(Self::Positioned(positions))
    }

    /// Returns the number of channels in the layout.
    pub fn channel_count(&self) -> ChannelCount {
        match self {
            Self::Discrete(count) => *count,
            Self::Positioned(positions) => {
                // Construction guarantees a non-empty length that fits u16.
                ChannelCount(positions.len() as u16)
            }
        }
    }

    /// Returns positioned channels, or `None` for a discrete layout.
    pub fn positions(&self) -> Option<&[ChannelPosition]> {
        match self {
            Self::Discrete(_) => None,
            Self::Positioned(positions) => Some(positions),
        }
    }
}

/// The position of a sample frame in one uninterrupted source stream.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FrameTimestamp {
    frame_index: u64,
    stream_time_ns: u64,
}

impl FrameTimestamp {
    /// Creates a timestamp from a zero-based frame index and monotonic stream
    /// time in nanoseconds.
    pub const fn new(frame_index: u64, stream_time_ns: u64) -> Self {
        Self {
            frame_index,
            stream_time_ns,
        }
    }

    /// Returns the zero-based frame index within this uninterrupted stream.
    pub const fn frame_index(self) -> u64 {
        self.frame_index
    }

    /// Returns nanoseconds elapsed in the stream's monotonic clock domain.
    pub const fn stream_time_ns(self) -> u64 {
        self.stream_time_ns
    }
}

/// A contiguous window on an uninterrupted source stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignalWindow {
    start: FrameTimestamp,
    frame_count: u32,
}

impl SignalWindow {
    /// Creates a non-empty window.
    pub fn new(start: FrameTimestamp, frame_count: u32) -> Result<Self, SignalDataError> {
        if frame_count == 0 {
            return Err(SignalDataError::EmptyFrame);
        }

        Ok(Self { start, frame_count })
    }

    pub const fn start(self) -> FrameTimestamp {
        self.start
    }

    pub const fn frame_count(self) -> u32 {
        self.frame_count
    }
}

/// A bounded batch of interleaved waveform samples.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioFrame {
    window: SignalWindow,
    sample_rate: SampleRate,
    channels: ChannelLayout,
    samples: Vec<Sample>,
}

impl AudioFrame {
    /// Creates a frame and validates its channel boundary and sample values.
    pub fn new(
        start: FrameTimestamp,
        sample_rate: SampleRate,
        channels: ChannelLayout,
        samples: Vec<Sample>,
    ) -> Result<Self, SignalDataError> {
        let channel_count = usize::from(channels.channel_count().get());
        if samples.is_empty() {
            return Err(SignalDataError::EmptyFrame);
        }
        if samples.len() % channel_count != 0 {
            return Err(SignalDataError::IncompleteSampleFrame {
                sample_count: samples.len(),
                channel_count: channels.channel_count(),
            });
        }
        if let Some((index, _)) = samples
            .iter()
            .enumerate()
            .find(|(_, sample)| !sample.is_finite())
        {
            return Err(SignalDataError::NonFiniteValue { index });
        }

        let frame_count = u32::try_from(samples.len() / channel_count)
            .map_err(|_| SignalDataError::TooManyFrames(samples.len() / channel_count))?;
        let window = SignalWindow::new(start, frame_count)?;

        Ok(Self {
            window,
            sample_rate,
            channels,
            samples,
        })
    }

    pub const fn window(&self) -> SignalWindow {
        self.window
    }

    pub const fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    pub fn channels(&self) -> &ChannelLayout {
        &self.channels
    }

    /// Returns samples in sample-frame-major, channel-minor order.
    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    pub fn into_samples(self) -> Vec<Sample> {
        self.samples
    }
}

/// Per-channel linear level measurements for a signal window.
///
/// RMS is `sqrt(mean(sample^2))`; peak is the maximum absolute sample. Both use
/// normalized full-scale units and are calculated independently per channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChannelLevel {
    rms: f32,
    peak: f32,
}

impl ChannelLevel {
    /// Creates non-negative RMS and sample-peak values in full-scale units.
    pub fn new(rms: f32, peak: f32) -> Result<Self, SignalDataError> {
        for (index, value) in [rms, peak].iter().enumerate() {
            if !value.is_finite() {
                return Err(SignalDataError::NonFiniteValue { index });
            }
            if *value < 0.0 {
                return Err(SignalDataError::NegativeMagnitude { index });
            }
        }
        if rms > peak {
            return Err(SignalDataError::RmsExceedsPeak { rms, peak });
        }

        Ok(Self { rms, peak })
    }

    pub const fn rms(self) -> f32 {
        self.rms
    }

    pub const fn peak(self) -> f32 {
        self.peak
    }
}

/// Provider-computed levels aligned to a source window.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelFrame {
    window: SignalWindow,
    channels: ChannelLayout,
    levels: Vec<ChannelLevel>,
}

impl LevelFrame {
    pub fn new(
        window: SignalWindow,
        channels: ChannelLayout,
        levels: Vec<ChannelLevel>,
    ) -> Result<Self, SignalDataError> {
        let expected = usize::from(channels.channel_count().get());
        if levels.len() != expected {
            return Err(SignalDataError::ChannelDataLength {
                expected,
                actual: levels.len(),
            });
        }

        Ok(Self {
            window,
            channels,
            levels,
        })
    }

    pub const fn window(&self) -> SignalWindow {
        self.window
    }

    pub fn channels(&self) -> &ChannelLayout {
        &self.channels
    }

    pub fn levels(&self) -> &[ChannelLevel] {
        &self.levels
    }
}

/// Window function applied before a magnitude spectrum is calculated.
///
/// `Rectangular` uses `w[n] = 1`. `Hann` uses the periodic form
/// `w[n] = 0.5 - 0.5 * cos(2 * pi * n / N)` for `0 <= n < N`.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SpectrumWindow {
    Rectangular,
    Hann,
}

/// A provider-computed, single-sided linear magnitude spectrum.
///
/// Magnitudes are channel-major. Each channel contains `fft_size / 2 + 1`
/// non-negative-frequency bins. Bin `n` is centered at
/// `n * sample_rate / fft_size` hertz. Values use coherent-gain-corrected peak
/// amplitude: magnitudes are divided by the sum of window coefficients; all
/// bins except DC and the even-sized FFT's Nyquist bin are then doubled. A
/// bin-centered sinusoid therefore reports its peak amplitude.
#[derive(Clone, Debug, PartialEq)]
pub struct SpectrumFrame {
    window: SignalWindow,
    sample_rate: SampleRate,
    channels: ChannelLayout,
    fft_size: u32,
    window_function: SpectrumWindow,
    magnitudes: Vec<f32>,
}

impl SpectrumFrame {
    pub fn new(
        window: SignalWindow,
        sample_rate: SampleRate,
        channels: ChannelLayout,
        fft_size: u32,
        window_function: SpectrumWindow,
        magnitudes: Vec<f32>,
    ) -> Result<Self, SignalDataError> {
        if fft_size == 0 {
            return Err(SignalDataError::ZeroFftSize);
        }
        if window_function == SpectrumWindow::Hann && window.frame_count() < 2 {
            return Err(SignalDataError::HannWindowTooShort {
                actual: window.frame_count(),
            });
        }
        if fft_size < window.frame_count() {
            return Err(SignalDataError::FftSmallerThanWindow {
                fft_size,
                window_frames: window.frame_count(),
            });
        }

        let bins_per_channel = (fft_size / 2 + 1) as usize;
        let expected = bins_per_channel
            .checked_mul(usize::from(channels.channel_count().get()))
            .ok_or(SignalDataError::SpectrumTooLarge)?;
        if magnitudes.len() != expected {
            return Err(SignalDataError::SpectrumDataLength {
                expected,
                actual: magnitudes.len(),
            });
        }
        for (index, magnitude) in magnitudes.iter().enumerate() {
            if !magnitude.is_finite() {
                return Err(SignalDataError::NonFiniteValue { index });
            }
            if *magnitude < 0.0 {
                return Err(SignalDataError::NegativeMagnitude { index });
            }
        }

        Ok(Self {
            window,
            sample_rate,
            channels,
            fft_size,
            window_function,
            magnitudes,
        })
    }

    pub const fn window(&self) -> SignalWindow {
        self.window
    }

    pub const fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    pub fn channels(&self) -> &ChannelLayout {
        &self.channels
    }

    pub const fn fft_size(&self) -> u32 {
        self.fft_size
    }

    pub const fn window_function(&self) -> SpectrumWindow {
        self.window_function
    }

    pub const fn bins_per_channel(&self) -> usize {
        (self.fft_size / 2 + 1) as usize
    }

    pub fn magnitudes_for_channel(&self, channel_index: usize) -> Option<&[f32]> {
        if channel_index >= usize::from(self.channels.channel_count().get()) {
            return None;
        }

        let bins = self.bins_per_channel();
        let start = channel_index * bins;
        Some(&self.magnitudes[start..start + bins])
    }

    /// Returns all magnitudes in channel-major order.
    pub fn magnitudes(&self) -> &[f32] {
        &self.magnitudes
    }

    pub fn into_magnitudes(self) -> Vec<f32> {
        self.magnitudes
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SignalFormatError {
    ZeroSampleRate,
    ZeroChannels,
    TooManyChannelPositions(usize),
    DuplicateChannelPosition(ChannelPosition),
    MonoWithOtherChannels,
}

impl fmt::Display for SignalFormatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroSampleRate => formatter.write_str("sample rate must be non-zero"),
            Self::ZeroChannels => formatter.write_str("channel count must be non-zero"),
            Self::TooManyChannelPositions(count) => {
                write!(formatter, "channel position count {count} exceeds u16")
            }
            Self::DuplicateChannelPosition(position) => {
                write!(
                    formatter,
                    "channel position {position:?} appears more than once"
                )
            }
            Self::MonoWithOtherChannels => {
                formatter.write_str("the mono position cannot be combined with other channels")
            }
        }
    }
}

impl Error for SignalFormatError {}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum SignalDataError {
    EmptyFrame,
    IncompleteSampleFrame {
        sample_count: usize,
        channel_count: ChannelCount,
    },
    TooManyFrames(usize),
    NonFiniteValue {
        index: usize,
    },
    NegativeMagnitude {
        index: usize,
    },
    RmsExceedsPeak {
        rms: f32,
        peak: f32,
    },
    ChannelDataLength {
        expected: usize,
        actual: usize,
    },
    ZeroFftSize,
    HannWindowTooShort {
        actual: u32,
    },
    FftSmallerThanWindow {
        fft_size: u32,
        window_frames: u32,
    },
    SpectrumDataLength {
        expected: usize,
        actual: usize,
    },
    SpectrumTooLarge,
}

impl fmt::Display for SignalDataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyFrame => formatter.write_str("signal frame must not be empty"),
            Self::IncompleteSampleFrame {
                sample_count,
                channel_count,
            } => write!(
                formatter,
                "{sample_count} samples do not contain complete {}-channel frames",
                channel_count.get()
            ),
            Self::TooManyFrames(count) => write!(formatter, "frame count {count} exceeds u32"),
            Self::NonFiniteValue { index } => {
                write!(formatter, "value at index {index} is not finite")
            }
            Self::NegativeMagnitude { index } => {
                write!(formatter, "magnitude at index {index} is negative")
            }
            Self::RmsExceedsPeak { rms, peak } => {
                write!(formatter, "RMS value {rms} exceeds peak value {peak}")
            }
            Self::ChannelDataLength { expected, actual } => write!(
                formatter,
                "channel data has length {actual}; expected {expected}"
            ),
            Self::ZeroFftSize => formatter.write_str("FFT size must be non-zero"),
            Self::HannWindowTooShort { actual } => write!(
                formatter,
                "periodic Hann window requires at least 2 frames; got {actual}"
            ),
            Self::FftSmallerThanWindow {
                fft_size,
                window_frames,
            } => write!(
                formatter,
                "FFT size {fft_size} is smaller than the {window_frames}-frame window"
            ),
            Self::SpectrumDataLength { expected, actual } => write!(
                formatter,
                "spectrum data has length {actual}; expected {expected}"
            ),
            Self::SpectrumTooLarge => formatter.write_str("spectrum size overflows usize"),
        }
    }
}

impl Error for SignalDataError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo() -> ChannelLayout {
        ChannelLayout::positioned([ChannelPosition::FrontLeft, ChannelPosition::FrontRight])
            .expect("stereo is valid")
    }

    #[test]
    fn audio_frame_preserves_interleaved_samples_and_boundaries() {
        let frame = AudioFrame::new(
            FrameTimestamp::new(128, 2_666_667),
            SampleRate::new(48_000).unwrap(),
            stereo(),
            vec![0.25, -0.25, 0.5, -0.5],
        )
        .unwrap();

        assert_eq!(frame.window().frame_count(), 2);
        assert_eq!(frame.window().start().frame_index(), 128);
        assert_eq!(frame.samples(), &[0.25, -0.25, 0.5, -0.5]);
    }

    #[test]
    fn audio_frame_rejects_partial_or_non_finite_data() {
        let partial = AudioFrame::new(
            FrameTimestamp::new(0, 0),
            SampleRate::new(48_000).unwrap(),
            stereo(),
            vec![0.0, 0.0, 0.5],
        );
        assert!(matches!(
            partial,
            Err(SignalDataError::IncompleteSampleFrame { .. })
        ));

        let non_finite = AudioFrame::new(
            FrameTimestamp::new(0, 0),
            SampleRate::new(48_000).unwrap(),
            stereo(),
            vec![0.0, f32::NAN],
        );
        assert_eq!(
            non_finite,
            Err(SignalDataError::NonFiniteValue { index: 1 })
        );
    }

    #[test]
    fn positioned_layout_rejects_ambiguous_positions() {
        assert_eq!(
            ChannelLayout::positioned([ChannelPosition::FrontLeft, ChannelPosition::FrontLeft,]),
            Err(SignalFormatError::DuplicateChannelPosition(
                ChannelPosition::FrontLeft
            ))
        );
        assert_eq!(
            ChannelLayout::positioned([ChannelPosition::Mono, ChannelPosition::FrontRight]),
            Err(SignalFormatError::MonoWithOtherChannels)
        );
    }

    #[test]
    fn levels_enforce_channel_count_and_physical_bounds() {
        assert_eq!(
            ChannelLevel::new(0.8, 0.7),
            Err(SignalDataError::RmsExceedsPeak {
                rms: 0.8,
                peak: 0.7
            })
        );

        let window = SignalWindow::new(FrameTimestamp::new(0, 0), 480).unwrap();
        let result = LevelFrame::new(window, stereo(), vec![ChannelLevel::new(0.1, 0.2).unwrap()]);
        assert_eq!(
            result,
            Err(SignalDataError::ChannelDataLength {
                expected: 2,
                actual: 1
            })
        );
    }

    #[test]
    fn spectrum_slices_channel_major_bins() {
        let window = SignalWindow::new(FrameTimestamp::new(0, 0), 4).unwrap();
        let spectrum = SpectrumFrame::new(
            window,
            SampleRate::new(48_000).unwrap(),
            stereo(),
            4,
            SpectrumWindow::Hann,
            vec![0.0, 0.5, 0.0, 0.0, 0.25, 0.0],
        )
        .unwrap();

        assert_eq!(spectrum.bins_per_channel(), 3);
        assert_eq!(
            spectrum.magnitudes_for_channel(1),
            Some(&[0.0, 0.25, 0.0][..])
        );
        assert_eq!(spectrum.magnitudes_for_channel(2), None);
    }

    #[test]
    fn spectrum_rejects_degenerate_hann_window() {
        let window = SignalWindow::new(FrameTimestamp::new(0, 0), 1).unwrap();
        let result = SpectrumFrame::new(
            window,
            SampleRate::new(48_000).unwrap(),
            ChannelLayout::positioned([ChannelPosition::Mono]).unwrap(),
            1,
            SpectrumWindow::Hann,
            vec![0.0],
        );

        assert_eq!(
            result,
            Err(SignalDataError::HannWindowTooShort { actual: 1 })
        );
    }
}
