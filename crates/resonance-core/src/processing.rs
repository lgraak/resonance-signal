//! Provider-independent, allocation-conscious signal-processing primitives.

use std::error::Error;
use std::fmt;

use crate::signal::{
    AudioFrame, ChannelLevel, FrameTimestamp, LevelFrame, Sample, SignalDataError, SignalWindow,
};

/// A borrowed, frame-aligned portion of an [`AudioFrame`].
///
/// The view does not copy waveform samples. It can represent an entire input
/// frame or a smaller contiguous window suitable for real-time level updates.
#[derive(Clone, Copy, Debug)]
pub struct WaveformWindow<'a> {
    frame: &'a AudioFrame,
    first_frame: u32,
    window: SignalWindow,
}

impl<'a> WaveformWindow<'a> {
    /// Borrows the complete waveform frame as one processing window.
    pub fn entire(frame: &'a AudioFrame) -> Self {
        Self {
            frame,
            first_frame: 0,
            window: frame.window(),
        }
    }

    /// Borrows a non-empty, frame-aligned subwindow.
    pub fn new(
        frame: &'a AudioFrame,
        first_frame: u32,
        frame_count: u32,
    ) -> Result<Self, ProcessingError> {
        let available = frame.window().frame_count();
        let end =
            first_frame
                .checked_add(frame_count)
                .ok_or(ProcessingError::WindowOutOfBounds {
                    first_frame,
                    frame_count,
                    available,
                })?;
        if frame_count == 0 {
            return Err(ProcessingError::EmptyInput);
        }
        if end > available {
            return Err(ProcessingError::WindowOutOfBounds {
                first_frame,
                frame_count,
                available,
            });
        }

        let source_start = frame.window().start();
        let frame_index = source_start
            .frame_index()
            .checked_add(u64::from(first_frame))
            .ok_or(ProcessingError::TimestampOverflow)?;
        let time_offset_ns = u64::from(first_frame)
            .checked_mul(1_000_000_000)
            .ok_or(ProcessingError::TimestampOverflow)?
            / u64::from(frame.sample_rate().hz());
        let stream_time_ns = source_start
            .stream_time_ns()
            .checked_add(time_offset_ns)
            .ok_or(ProcessingError::TimestampOverflow)?;
        let window = SignalWindow::new(
            FrameTimestamp::new(frame_index, stream_time_ns),
            frame_count,
        )
        .map_err(ProcessingError::InvalidSignalData)?;

        Ok(Self {
            frame,
            first_frame,
            window,
        })
    }

    /// Returns the source window represented by this view.
    pub const fn signal_window(self) -> SignalWindow {
        self.window
    }

    /// Returns interleaved samples in sample-frame-major order.
    pub fn samples(self) -> &'a [Sample] {
        let channel_count = usize::from(self.frame.channels().channel_count().get());
        let start = self.first_frame as usize * channel_count;
        let end = start + self.window.frame_count() as usize * channel_count;
        &self.frame.samples()[start..end]
    }

    /// Iterates samples from one channel without deinterleaving or allocation.
    pub fn samples_for_channel(
        self,
        channel_index: usize,
    ) -> Option<impl Iterator<Item = Sample> + 'a> {
        let channel_count = usize::from(self.frame.channels().channel_count().get());
        (channel_index < channel_count).then(|| {
            self.samples()[channel_index..]
                .iter()
                .step_by(channel_count)
                .copied()
        })
    }
}

/// Calculates RMS for one non-empty series of finite samples.
pub fn rms(samples: &[Sample]) -> Result<f32, ProcessingError> {
    validate_samples(samples)?;

    let sum_of_squares = samples
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum::<f64>();
    Ok((sum_of_squares / samples.len() as f64).sqrt() as f32)
}

/// Calculates the maximum absolute magnitude of finite samples.
///
/// Values outside nominal full scale are preserved rather than clipped.
pub fn peak(samples: &[Sample]) -> Result<f32, ProcessingError> {
    validate_samples(samples)?;

    Ok(samples
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0_f32, f32::max))
}

/// Calculates per-channel RMS and absolute sample peak in one interleaved pass.
pub fn calculate_levels(window: WaveformWindow<'_>) -> Result<LevelFrame, ProcessingError> {
    let channel_count = usize::from(window.frame.channels().channel_count().get());
    let mut sum_of_squares = vec![0.0_f64; channel_count];
    let mut peaks = vec![0.0_f32; channel_count];

    for (sample_index, sample) in window.samples().iter().copied().enumerate() {
        if !sample.is_finite() {
            return Err(ProcessingError::NonFiniteSample {
                index: sample_index,
            });
        }

        let channel_index = sample_index % channel_count;
        sum_of_squares[channel_index] += f64::from(sample).powi(2);
        peaks[channel_index] = peaks[channel_index].max(sample.abs());
    }

    let sample_frames = f64::from(window.signal_window().frame_count());
    let levels = sum_of_squares
        .into_iter()
        .zip(peaks)
        .map(|(sum, peak)| {
            // f64 accumulation avoids overflow for every finite f32 input. The
            // minimum compensates only for a possible final rounding ulp.
            let rms = ((sum / sample_frames).sqrt() as f32).min(peak);
            ChannelLevel::new(rms, peak).map_err(ProcessingError::InvalidSignalData)
        })
        .collect::<Result<Vec<_>, _>>()?;

    LevelFrame::new(
        window.signal_window(),
        window.frame.channels().clone(),
        levels,
    )
    .map_err(ProcessingError::InvalidSignalData)
}

/// Returns the gain that moves the current absolute peak to `target_peak`.
///
/// Silence returns unity gain. `target_peak` may exceed `1.0` when a caller
/// intentionally wants to retain or create headroom outside nominal range.
pub fn peak_normalization_gain(
    samples: &[Sample],
    target_peak: f32,
) -> Result<f64, ProcessingError> {
    validate_target_peak(target_peak)?;
    let current_peak = peak(samples)?;

    if current_peak == 0.0 {
        Ok(1.0)
    } else {
        Ok(f64::from(target_peak) / f64::from(current_peak))
    }
}

/// Applies peak normalization after validating the complete slice.
///
/// Validation happens before mutation, so an error never leaves partially
/// normalized data behind. The applied gain is returned.
pub fn normalize_peak_in_place(
    samples: &mut [Sample],
    target_peak: f32,
) -> Result<f64, ProcessingError> {
    let gain = peak_normalization_gain(samples, target_peak)?;
    for sample in samples {
        *sample = (f64::from(*sample) * gain) as f32;
    }
    Ok(gain)
}

fn validate_samples(samples: &[Sample]) -> Result<(), ProcessingError> {
    if samples.is_empty() {
        return Err(ProcessingError::EmptyInput);
    }
    if let Some((index, _)) = samples
        .iter()
        .enumerate()
        .find(|(_, sample)| !sample.is_finite())
    {
        return Err(ProcessingError::NonFiniteSample { index });
    }

    Ok(())
}

fn validate_target_peak(target_peak: f32) -> Result<(), ProcessingError> {
    if !target_peak.is_finite() || target_peak < 0.0 {
        return Err(ProcessingError::InvalidTargetPeak(target_peak));
    }

    Ok(())
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum ProcessingError {
    EmptyInput,
    NonFiniteSample {
        index: usize,
    },
    WindowOutOfBounds {
        first_frame: u32,
        frame_count: u32,
        available: u32,
    },
    TimestampOverflow,
    InvalidTargetPeak(f32),
    InvalidSignalData(SignalDataError),
}

impl fmt::Display for ProcessingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyInput => formatter.write_str("processing input must not be empty"),
            Self::NonFiniteSample { index } => {
                write!(formatter, "sample at index {index} is not finite")
            }
            Self::WindowOutOfBounds {
                first_frame,
                frame_count,
                available,
            } => write!(
                formatter,
                "window starting at frame {first_frame} with {frame_count} frames exceeds {available} available frames"
            ),
            Self::TimestampOverflow => formatter.write_str("waveform window timestamp overflowed"),
            Self::InvalidTargetPeak(value) => write!(
                formatter,
                "normalization target peak must be finite and non-negative; got {value}"
            ),
            Self::InvalidSignalData(error) => error.fmt(formatter),
        }
    }
}

impl Error for ProcessingError {
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
    use crate::signal::{ChannelLayout, ChannelPosition, SampleRate};

    fn mono(samples: Vec<Sample>) -> Result<AudioFrame, SignalDataError> {
        AudioFrame::new(
            FrameTimestamp::new(100, 2_000_000),
            SampleRate::new(48_000).unwrap(),
            ChannelLayout::positioned([ChannelPosition::Mono]).unwrap(),
            samples,
        )
    }

    fn stereo(samples: Vec<Sample>) -> Result<AudioFrame, SignalDataError> {
        AudioFrame::new(
            FrameTimestamp::new(100, 2_000_000),
            SampleRate::new(48_000).unwrap(),
            ChannelLayout::positioned([ChannelPosition::FrontLeft, ChannelPosition::FrontRight])
                .unwrap(),
            samples,
        )
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < 1.0e-6, "{actual} != {expected}");
    }

    #[test]
    fn silence_has_zero_rms_and_peak() {
        let frame = mono(vec![0.0; 8]).unwrap();
        let levels = calculate_levels(WaveformWindow::entire(&frame)).unwrap();

        assert_eq!(levels.levels()[0].rms(), 0.0);
        assert_eq!(levels.levels()[0].peak(), 0.0);
    }

    #[test]
    fn constant_signal_has_matching_rms_and_peak() {
        assert_eq!(rms(&[0.5; 4]).unwrap(), 0.5);
        assert_eq!(peak(&[-0.5; 4]).unwrap(), 0.5);
    }

    #[test]
    fn known_bipolar_waveform_has_expected_rms() {
        let samples = [1.0, -1.0, 0.0, 0.0];

        assert_close(rms(&samples).unwrap(), std::f32::consts::FRAC_1_SQRT_2);
        assert_eq!(peak(&samples).unwrap(), 1.0);
    }

    #[test]
    fn levels_respect_interleaved_channel_layout_and_headroom() {
        let frame = stereo(vec![1.0, -0.25, -1.0, 0.25, 0.0, -1.5, 0.0, 1.5]).unwrap();
        let levels = calculate_levels(WaveformWindow::entire(&frame)).unwrap();

        assert_eq!(levels.channels(), frame.channels());
        assert_close(levels.levels()[0].rms(), std::f32::consts::FRAC_1_SQRT_2);
        assert_eq!(levels.levels()[0].peak(), 1.0);
        assert_close(levels.levels()[1].rms(), 1.15625_f32.sqrt());
        assert_eq!(levels.levels()[1].peak(), 1.5);
    }

    #[test]
    fn subwindow_is_zero_copy_and_preserves_source_alignment() {
        let frame = stereo(vec![0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7]).unwrap();
        let window = WaveformWindow::new(&frame, 1, 2).unwrap();

        assert_eq!(window.samples(), &[0.2, 0.3, 0.4, 0.5]);
        assert_eq!(
            window.samples_for_channel(1).unwrap().collect::<Vec<_>>(),
            vec![0.3, 0.5]
        );
        assert_eq!(
            window.samples_for_channel(2).map(|samples| samples.count()),
            None
        );
        assert_eq!(window.signal_window().start().frame_index(), 101);
        assert_eq!(window.signal_window().start().stream_time_ns(), 2_020_833);
    }

    #[test]
    fn empty_and_out_of_bounds_inputs_are_rejected() {
        assert_eq!(rms(&[]), Err(ProcessingError::EmptyInput));
        assert_eq!(peak(&[]), Err(ProcessingError::EmptyInput));

        let frame = mono(vec![0.0, 0.0]).unwrap();
        assert!(matches!(
            WaveformWindow::new(&frame, 0, 0),
            Err(ProcessingError::EmptyInput)
        ));
        assert!(matches!(
            WaveformWindow::new(&frame, 1, 2),
            Err(ProcessingError::WindowOutOfBounds { .. })
        ));
    }

    #[test]
    fn non_finite_samples_are_rejected_before_processing_or_mutation() {
        assert_eq!(
            rms(&[0.0, f32::NAN]),
            Err(ProcessingError::NonFiniteSample { index: 1 })
        );

        let mut samples = [0.25, f32::INFINITY];
        let original = samples;
        assert_eq!(
            normalize_peak_in_place(&mut samples, 1.0),
            Err(ProcessingError::NonFiniteSample { index: 1 })
        );
        assert_eq!(samples, original);
    }

    #[test]
    fn peak_normalization_handles_silence_and_preserves_shape() {
        let mut silence = [0.0; 4];
        assert_eq!(normalize_peak_in_place(&mut silence, 1.0).unwrap(), 1.0);
        assert_eq!(silence, [0.0; 4]);

        let mut samples = [-0.5, 0.25, 1.0];
        assert_eq!(normalize_peak_in_place(&mut samples, 2.0).unwrap(), 2.0);
        assert_eq!(samples, [-1.0, 0.5, 2.0]);
    }

    #[test]
    fn invalid_normalization_target_is_rejected() {
        assert!(matches!(
            peak_normalization_gain(&[0.5], f32::NAN),
            Err(ProcessingError::InvalidTargetPeak(value)) if value.is_nan()
        ));
        assert_eq!(
            peak_normalization_gain(&[0.5], -0.1),
            Err(ProcessingError::InvalidTargetPeak(-0.1))
        );
    }
}
