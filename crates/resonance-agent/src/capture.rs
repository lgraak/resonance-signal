//! Hardware-independent capture packet validation and frame generation.

use std::error::Error;
use std::fmt;
use std::time::Duration;

use resonance_api::contract::{
    AudioFrame, ChannelLayout, ChannelPosition, FrameTimestamp, SampleRate, SignalDataError,
    SignalFormatError,
};

const BYTES_PER_F32: usize = std::mem::size_of::<f32>();
const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// The fixed format selected at a platform capture boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureFormat {
    sample_rate: SampleRate,
    channels: ChannelLayout,
}

impl CaptureFormat {
    /// Creates the mono or canonical front-left/front-right format emitted by
    /// the Windows capture boundary.
    pub fn mono_or_stereo(sample_rate_hz: u32, channel_count: u16) -> Result<Self, CaptureError> {
        let sample_rate = SampleRate::new(sample_rate_hz).map_err(CaptureError::InvalidFormat)?;
        let channels = match channel_count {
            1 => ChannelLayout::positioned([ChannelPosition::Mono]),
            2 => {
                ChannelLayout::positioned([ChannelPosition::FrontLeft, ChannelPosition::FrontRight])
            }
            actual => return Err(CaptureError::UnsupportedChannelCount(actual)),
        }
        .map_err(CaptureError::InvalidFormat)?;

        Ok(Self {
            sample_rate,
            channels,
        })
    }

    pub const fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    pub fn channels(&self) -> &ChannelLayout {
        &self.channels
    }

    pub fn channel_count(&self) -> u16 {
        self.channels.channel_count().get()
    }

    pub fn bytes_per_frame(&self) -> usize {
        usize::from(self.channel_count()) * BYTES_PER_F32
    }
}

/// WASAPI truth signals associated with one captured packet.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PacketFlags {
    pub data_discontinuity: bool,
    pub silent: bool,
    pub timestamp_error: bool,
}

/// One bounded native packet copied out of the real-time capture path.
#[derive(Debug)]
pub struct CapturePacket {
    buffer: Vec<u8>,
    byte_len: usize,
    frame_count: u32,
    device_position: u64,
    qpc_timestamp_100ns: u64,
    flags: PacketFlags,
    callback_interval: Option<Duration>,
    callback_duration: Duration,
}

impl CapturePacket {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        buffer: Vec<u8>,
        byte_len: usize,
        frame_count: u32,
        device_position: u64,
        qpc_timestamp_100ns: u64,
        flags: PacketFlags,
        callback_interval: Option<Duration>,
        callback_duration: Duration,
    ) -> Result<Self, CaptureError> {
        if byte_len > buffer.len() {
            return Err(CaptureError::PacketLengthExceedsBuffer {
                byte_len,
                buffer_len: buffer.len(),
            });
        }

        Ok(Self {
            buffer,
            byte_len,
            frame_count,
            device_position,
            qpc_timestamp_100ns,
            flags,
            callback_interval,
            callback_duration,
        })
    }

    pub const fn frame_count(&self) -> u32 {
        self.frame_count
    }

    pub const fn device_position(&self) -> u64 {
        self.device_position
    }

    pub const fn qpc_timestamp_100ns(&self) -> u64 {
        self.qpc_timestamp_100ns
    }

    pub const fn flags(&self) -> PacketFlags {
        self.flags
    }

    pub const fn callback_interval(&self) -> Option<Duration> {
        self.callback_interval
    }

    pub const fn callback_duration(&self) -> Duration {
        self.callback_duration
    }

    pub fn into_buffer(self) -> Vec<u8> {
        self.buffer
    }

    fn bytes(&self) -> &[u8] {
        &self.buffer[..self.byte_len]
    }
}

/// Additional timing evidence retained while constructing an [`AudioFrame`].
#[derive(Debug)]
pub struct BuiltAudioFrame {
    frame: AudioFrame,
    qpc_delta: Option<Duration>,
    initial_discontinuity: bool,
}

impl BuiltAudioFrame {
    pub fn frame(&self) -> &AudioFrame {
        &self.frame
    }

    pub fn into_frame(self) -> AudioFrame {
        self.frame
    }

    pub const fn qpc_delta(&self) -> Option<Duration> {
        self.qpc_delta
    }

    pub const fn initial_discontinuity(&self) -> bool {
        self.initial_discontinuity
    }
}

/// Stateful conversion seam between native capture packets and provider frames.
///
/// WASAPI device positions prove packet continuity. QPC timestamps are retained
/// and checked for monotonicity. Provider timestamps are derived from the
/// contiguous source-frame position and negotiated sample rate so they remain
/// in the audio stream's clock domain rather than the callback scheduler's.
#[derive(Debug)]
pub struct AudioFrameBuilder {
    format: CaptureFormat,
    next_device_position: Option<u64>,
    next_frame_index: u64,
    previous_qpc_timestamp_100ns: Option<u64>,
}

impl AudioFrameBuilder {
    pub fn new(format: CaptureFormat) -> Self {
        Self {
            format,
            next_device_position: None,
            next_frame_index: 0,
            previous_qpc_timestamp_100ns: None,
        }
    }

    pub fn format(&self) -> &CaptureFormat {
        &self.format
    }

    pub fn push(&mut self, packet: &CapturePacket) -> Result<BuiltAudioFrame, CaptureError> {
        if packet.frame_count == 0 {
            return Err(CaptureError::EmptyPacket);
        }
        if packet.flags.timestamp_error {
            return Err(CaptureError::TimestampUnavailable);
        }

        let first_packet = self.next_device_position.is_none();
        if !first_packet && packet.flags.data_discontinuity {
            return Err(CaptureError::DataDiscontinuity);
        }
        if let Some(expected) = self.next_device_position {
            if packet.device_position != expected {
                return Err(CaptureError::DevicePositionDiscontinuity {
                    expected,
                    actual: packet.device_position,
                });
            }
        }

        let qpc_delta = if let Some(previous) = self.previous_qpc_timestamp_100ns {
            let elapsed = packet.qpc_timestamp_100ns.checked_sub(previous).ok_or(
                CaptureError::NonMonotonicQpc {
                    previous,
                    actual: packet.qpc_timestamp_100ns,
                },
            )?;
            Some(Duration::from_nanos(
                elapsed
                    .checked_mul(100)
                    .ok_or(CaptureError::TimestampOverflow)?,
            ))
        } else {
            None
        };

        let expected_bytes = usize::try_from(packet.frame_count)
            .ok()
            .and_then(|frames| frames.checked_mul(self.format.bytes_per_frame()))
            .ok_or(CaptureError::PacketSizeOverflow)?;
        if packet.bytes().len() != expected_bytes {
            return Err(CaptureError::UnexpectedPacketLength {
                expected: expected_bytes,
                actual: packet.bytes().len(),
            });
        }

        let sample_count = usize::try_from(packet.frame_count)
            .ok()
            .and_then(|frames| frames.checked_mul(usize::from(self.format.channel_count())))
            .ok_or(CaptureError::PacketSizeOverflow)?;
        let samples = if packet.flags.silent {
            vec![0.0; sample_count]
        } else {
            decode_f32_le(packet.bytes())?
        };

        let stream_time_ns = frame_time_ns(self.next_frame_index, self.format.sample_rate)?;
        let frame = AudioFrame::new(
            FrameTimestamp::new(self.next_frame_index, stream_time_ns),
            self.format.sample_rate,
            self.format.channels.clone(),
            samples,
        )
        .map_err(CaptureError::InvalidAudioFrame)?;

        self.next_device_position = Some(
            packet
                .device_position
                .checked_add(u64::from(packet.frame_count))
                .ok_or(CaptureError::DevicePositionOverflow)?,
        );
        self.next_frame_index = self
            .next_frame_index
            .checked_add(u64::from(packet.frame_count))
            .ok_or(CaptureError::FrameIndexOverflow)?;
        self.previous_qpc_timestamp_100ns = Some(packet.qpc_timestamp_100ns);

        Ok(BuiltAudioFrame {
            frame,
            qpc_delta,
            initial_discontinuity: first_packet && packet.flags.data_discontinuity,
        })
    }
}

fn decode_f32_le(bytes: &[u8]) -> Result<Vec<f32>, CaptureError> {
    if !bytes.len().is_multiple_of(BYTES_PER_F32) {
        return Err(CaptureError::IncompleteFloatSample(bytes.len()));
    }

    let mut samples = Vec::with_capacity(bytes.len() / BYTES_PER_F32);
    let (chunks, remainder) = bytes.as_chunks::<BYTES_PER_F32>();
    debug_assert!(remainder.is_empty());
    for chunk in chunks {
        let sample = f32::from_le_bytes(*chunk);
        if !sample.is_finite() {
            return Err(CaptureError::NonFiniteSample(samples.len()));
        }
        samples.push(sample);
    }
    Ok(samples)
}

fn frame_time_ns(frame_index: u64, sample_rate: SampleRate) -> Result<u64, CaptureError> {
    let nanos = u128::from(frame_index)
        .checked_mul(NANOS_PER_SECOND)
        .ok_or(CaptureError::TimestampOverflow)?
        / u128::from(sample_rate.hz());
    u64::try_from(nanos).map_err(|_| CaptureError::TimestampOverflow)
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum CaptureError {
    UnsupportedChannelCount(u16),
    InvalidFormat(SignalFormatError),
    InvalidAudioFrame(SignalDataError),
    PacketLengthExceedsBuffer { byte_len: usize, buffer_len: usize },
    UnexpectedPacketLength { expected: usize, actual: usize },
    IncompleteFloatSample(usize),
    NonFiniteSample(usize),
    EmptyPacket,
    TimestampUnavailable,
    DataDiscontinuity,
    DevicePositionDiscontinuity { expected: u64, actual: u64 },
    NonMonotonicQpc { previous: u64, actual: u64 },
    PacketSizeOverflow,
    DevicePositionOverflow,
    FrameIndexOverflow,
    TimestampOverflow,
}

impl fmt::Display for CaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedChannelCount(actual) => {
                write!(
                    formatter,
                    "capture supports one or two channels; got {actual}"
                )
            }
            Self::InvalidFormat(error) => error.fmt(formatter),
            Self::InvalidAudioFrame(error) => error.fmt(formatter),
            Self::PacketLengthExceedsBuffer {
                byte_len,
                buffer_len,
            } => write!(
                formatter,
                "packet length {byte_len} exceeds its {buffer_len}-byte backing buffer"
            ),
            Self::UnexpectedPacketLength { expected, actual } => write!(
                formatter,
                "packet contains {actual} bytes; expected exactly {expected}"
            ),
            Self::IncompleteFloatSample(actual) => {
                write!(
                    formatter,
                    "packet byte length {actual} is not aligned to f32"
                )
            }
            Self::NonFiniteSample(index) => {
                write!(formatter, "captured sample at index {index} is not finite")
            }
            Self::EmptyPacket => formatter.write_str("capture packet contains no frames"),
            Self::TimestampUnavailable => {
                formatter.write_str("WASAPI marked the packet timestamp invalid")
            }
            Self::DataDiscontinuity => formatter.write_str("WASAPI reported a data discontinuity"),
            Self::DevicePositionDiscontinuity { expected, actual } => write!(
                formatter,
                "WASAPI device position is discontinuous: expected {expected}, got {actual}"
            ),
            Self::NonMonotonicQpc { previous, actual } => write!(
                formatter,
                "WASAPI QPC timestamp moved backward from {previous} to {actual}"
            ),
            Self::PacketSizeOverflow => formatter.write_str("capture packet size overflowed"),
            Self::DevicePositionOverflow => {
                formatter.write_str("WASAPI device position overflowed")
            }
            Self::FrameIndexOverflow => formatter.write_str("stream frame index overflowed"),
            Self::TimestampOverflow => formatter.write_str("stream timestamp overflowed"),
        }
    }
}

impl Error for CaptureError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidFormat(error) => Some(error),
            Self::InvalidAudioFrame(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(
        samples: &[f32],
        frame_count: u32,
        device_position: u64,
        qpc_timestamp_100ns: u64,
        flags: PacketFlags,
    ) -> CapturePacket {
        let mut bytes = Vec::with_capacity(samples.len() * BYTES_PER_F32);
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        let byte_len = bytes.len();
        CapturePacket::new(
            bytes,
            byte_len,
            frame_count,
            device_position,
            qpc_timestamp_100ns,
            flags,
            None,
            Duration::ZERO,
        )
        .unwrap()
    }

    #[test]
    fn converts_interleaved_stereo_and_generates_contiguous_frames() {
        let format = CaptureFormat::mono_or_stereo(48_000, 2).unwrap();
        let mut builder = AudioFrameBuilder::new(format);

        let first = builder
            .push(&packet(
                &[0.25, -0.25, 0.5, -0.5],
                2,
                900,
                50_000,
                PacketFlags::default(),
            ))
            .unwrap();
        let second = builder
            .push(&packet(
                &[0.75, -0.75],
                1,
                902,
                50_417,
                PacketFlags::default(),
            ))
            .unwrap();

        assert_eq!(first.frame().samples(), &[0.25, -0.25, 0.5, -0.5]);
        assert_eq!(first.frame().window().start(), FrameTimestamp::new(0, 0));
        assert_eq!(second.frame().window().start().frame_index(), 2);
        assert_eq!(second.frame().window().start().stream_time_ns(), 41_666);
        assert_eq!(second.qpc_delta(), Some(Duration::from_nanos(41_700)));
    }

    #[test]
    fn silent_packet_produces_explicit_finite_zeroes() {
        let format = CaptureFormat::mono_or_stereo(44_100, 1).unwrap();
        let mut builder = AudioFrameBuilder::new(format);
        let frame = builder
            .push(&packet(
                &[f32::NAN, f32::INFINITY],
                2,
                10,
                20,
                PacketFlags {
                    silent: true,
                    ..PacketFlags::default()
                },
            ))
            .unwrap();

        assert_eq!(frame.frame().samples(), &[0.0, 0.0]);
    }

    #[test]
    fn rejects_unsupported_layouts_and_invalid_sample_data() {
        assert_eq!(
            CaptureFormat::mono_or_stereo(48_000, 6),
            Err(CaptureError::UnsupportedChannelCount(6))
        );

        let format = CaptureFormat::mono_or_stereo(48_000, 1).unwrap();
        let mut builder = AudioFrameBuilder::new(format);
        assert!(matches!(
            builder.push(&packet(&[f32::NAN], 1, 0, 0, PacketFlags::default())),
            Err(CaptureError::NonFiniteSample(0))
        ));
    }

    #[test]
    fn rejects_packet_discontinuity_and_invalid_timing() {
        let format = CaptureFormat::mono_or_stereo(48_000, 1).unwrap();
        let mut builder = AudioFrameBuilder::new(format.clone());
        builder
            .push(&packet(&[0.0], 1, 100, 1_000, PacketFlags::default()))
            .unwrap();

        assert!(matches!(
            builder.push(&packet(&[0.0], 1, 102, 1_100, PacketFlags::default())),
            Err(CaptureError::DevicePositionDiscontinuity {
                expected: 101,
                actual: 102
            })
        ));

        let mut builder = AudioFrameBuilder::new(format);
        assert!(matches!(
            builder.push(&packet(
                &[0.0],
                1,
                0,
                0,
                PacketFlags {
                    timestamp_error: true,
                    ..PacketFlags::default()
                }
            )),
            Err(CaptureError::TimestampUnavailable)
        ));

        let mut builder = AudioFrameBuilder::new(CaptureFormat::mono_or_stereo(48_000, 1).unwrap());
        builder
            .push(&packet(&[0.0], 1, 10, 500, PacketFlags::default()))
            .unwrap();
        assert!(matches!(
            builder.push(&packet(&[0.0], 1, 11, 499, PacketFlags::default())),
            Err(CaptureError::NonMonotonicQpc {
                previous: 500,
                actual: 499
            })
        ));
    }

    #[test]
    fn initial_discontinuity_starts_a_new_stream_but_later_one_ends_it() {
        let format = CaptureFormat::mono_or_stereo(48_000, 1).unwrap();
        let mut builder = AudioFrameBuilder::new(format);
        let first = builder
            .push(&packet(
                &[0.0],
                1,
                10,
                100,
                PacketFlags {
                    data_discontinuity: true,
                    ..PacketFlags::default()
                },
            ))
            .unwrap();
        assert!(first.initial_discontinuity());

        assert!(matches!(
            builder.push(&packet(
                &[0.0],
                1,
                11,
                200,
                PacketFlags {
                    data_discontinuity: true,
                    ..PacketFlags::default()
                }
            )),
            Err(CaptureError::DataDiscontinuity)
        ));
    }
}
