//! Background-noise suppression for the microphone, using RNNoise via `nnnoiseless`.
//!
//! RNNoise is the same class of filter as Discord's own Krisp: a small recurrent network
//! that keeps speech while removing steady noise (fans, hiss) and transient clatter
//! (keyboards, clicks). The model weights are compiled into the binary, so this adds
//! nothing to carry alongside the exe.
//!
//! RNNoise works on a single channel of 48kHz audio in fixed 480-sample (10ms) frames,
//! and it is stateful — each frame's result depends on the ones before it. This wraps
//! that to accept the pipeline's interleaved-stereo byte chunks of any size: it buffers
//! each channel until a full frame is ready, denoises the channels in lockstep, and
//! re-interleaves the result. Up to one frame (10ms) can be held back between calls.

use nnnoiseless::DenoiseState;

/// 480 samples — 10ms at the capture rate.
const FRAME_SIZE: usize = DenoiseState::FRAME_SIZE;
const BYTES_PER_SAMPLE: usize = 2;

/// Cleans a stream of interleaved 16-bit PCM, one RNNoise instance per channel.
pub struct StreamDenoiser {
    channels: Vec<ChannelDenoiser>,
}

struct ChannelDenoiser {
    state: Box<DenoiseState<'static>>,
    /// Samples waiting for enough to fill a frame.
    pending: Vec<f32>,
}

impl ChannelDenoiser {
    fn new() -> Self {
        Self {
            state: DenoiseState::new(),
            pending: Vec::with_capacity(FRAME_SIZE * 2),
        }
    }
}

impl StreamDenoiser {
    /// One denoiser for audio with the given number of interleaved channels.
    pub fn new(channel_count: u16) -> Self {
        let channel_count = usize::from(channel_count.max(1));
        let channels = (0..channel_count).map(|_| ChannelDenoiser::new()).collect();

        Self { channels }
    }

    /// Denoises a chunk of interleaved 16-bit little-endian PCM.
    ///
    /// The returned audio is whatever whole frames are ready; a trailing partial frame is
    /// held until the next call. Early on that means an empty result, which the caller
    /// treats as "nothing to process yet".
    pub fn process(&mut self, pcm: &[u8]) -> Vec<u8> {
        self.buffer_samples(pcm);

        let frames_ready = self.frames_ready();
        if frames_ready == 0 {
            return Vec::new();
        }

        let denoised_channels = self.denoise_ready_frames(frames_ready);
        interleave(&denoised_channels)
    }

    /// De-interleaves the chunk into each channel's pending buffer.
    fn buffer_samples(&mut self, pcm: &[u8]) {
        let channel_count = self.channels.len();

        for (index, sample_bytes) in pcm.chunks_exact(BYTES_PER_SAMPLE).enumerate() {
            let sample = i16::from_le_bytes([sample_bytes[0], sample_bytes[1]]);
            // RNNoise works in i16-magnitude floats, not the normalised -1.0..1.0 range.
            self.channels[index % channel_count]
                .pending
                .push(f32::from(sample));
        }
    }

    /// How many whole frames every channel can produce. The channels are fed in lockstep,
    /// so this is the same for each, but taking the minimum keeps them aligned even if a
    /// chunk ever arrived with a partial interleaved sample.
    fn frames_ready(&self) -> usize {
        self.channels
            .iter()
            .map(|channel| channel.pending.len() / FRAME_SIZE)
            .min()
            .unwrap_or(0)
    }

    /// Runs the model over the ready frames of every channel, draining what it consumes.
    fn denoise_ready_frames(&mut self, frames_ready: usize) -> Vec<Vec<f32>> {
        let samples_to_process = frames_ready * FRAME_SIZE;
        let mut denoised_channels = Vec::with_capacity(self.channels.len());

        for channel in &mut self.channels {
            let mut output = vec![0.0_f32; samples_to_process];
            let mut input_frame = [0.0_f32; FRAME_SIZE];
            let mut output_frame = [0.0_f32; FRAME_SIZE];

            for frame_index in 0..frames_ready {
                let start = frame_index * FRAME_SIZE;
                input_frame.copy_from_slice(&channel.pending[start..start + FRAME_SIZE]);
                channel.state.process_frame(&mut output_frame, &input_frame);
                output[start..start + FRAME_SIZE].copy_from_slice(&output_frame);
            }

            channel.pending.drain(0..samples_to_process);
            denoised_channels.push(output);
        }

        denoised_channels
    }
}

/// Weaves per-channel samples back into one interleaved 16-bit PCM byte stream.
fn interleave(channels: &[Vec<f32>]) -> Vec<u8> {
    let Some(samples_per_channel) = channels.first().map(|channel| channel.len()) else {
        return Vec::new();
    };

    let mut bytes = Vec::with_capacity(samples_per_channel * channels.len() * BYTES_PER_SAMPLE);

    for sample_index in 0..samples_per_channel {
        for channel in channels {
            let sample = to_i16(channel[sample_index]);
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
    }

    bytes
}

/// Rounds a processed float back to a sample, clamped first so the cast cannot truncate.
fn to_i16(sample: f32) -> i16 {
    let clamped = sample
        .round()
        .clamp(f32::from(i16::MIN), f32::from(i16::MAX));
    clamped as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHANNEL_COUNT: u16 = 2;

    /// Builds a chunk of interleaved stereo PCM of the given per-channel sample count.
    fn stereo_chunk(samples_per_channel: usize, amplitude: i16) -> Vec<u8> {
        let mut bytes = Vec::new();

        for _ in 0..samples_per_channel {
            // Same value in both channels, twice per frame position.
            bytes.extend_from_slice(&amplitude.to_le_bytes());
            bytes.extend_from_slice(&amplitude.to_le_bytes());
        }

        bytes
    }

    #[test]
    fn less_than_a_frame_is_held_back() {
        let mut denoiser = StreamDenoiser::new(CHANNEL_COUNT);
        let half_frame = stereo_chunk(FRAME_SIZE / 2, 1_000);

        assert!(denoiser.process(&half_frame).is_empty());
    }

    #[test]
    fn a_whole_frame_comes_back_as_a_whole_frame() {
        let mut denoiser = StreamDenoiser::new(CHANNEL_COUNT);
        let one_frame = stereo_chunk(FRAME_SIZE, 1_000);

        let output = denoiser.process(&one_frame);

        // Same size in as out: one stereo frame of 16-bit samples.
        assert_eq!(output.len(), FRAME_SIZE * 2 * BYTES_PER_SAMPLE);
    }

    #[test]
    fn two_half_frames_join_into_one_output_frame() {
        let mut denoiser = StreamDenoiser::new(CHANNEL_COUNT);
        let half_frame = stereo_chunk(FRAME_SIZE / 2, 1_000);

        assert!(denoiser.process(&half_frame).is_empty());
        let output = denoiser.process(&half_frame);

        assert_eq!(output.len(), FRAME_SIZE * 2 * BYTES_PER_SAMPLE);
    }

    #[test]
    fn output_is_always_whole_frames() {
        let mut denoiser = StreamDenoiser::new(CHANNEL_COUNT);
        // A frame and a half: half a frame should be carried over.
        let chunk = stereo_chunk(FRAME_SIZE + FRAME_SIZE / 2, 2_000);

        let output = denoiser.process(&chunk);
        let bytes_per_frame = FRAME_SIZE * 2 * BYTES_PER_SAMPLE;

        assert_eq!(output.len(), bytes_per_frame);
    }

    #[test]
    fn silence_denoises_to_silence() {
        let mut denoiser = StreamDenoiser::new(CHANNEL_COUNT);
        let silence = stereo_chunk(FRAME_SIZE, 0);

        let output = denoiser.process(&silence);

        assert!(output.iter().all(|&byte| byte == 0));
    }

    #[test]
    fn a_mono_stream_is_handled_too() {
        let mut denoiser = StreamDenoiser::new(1);
        let mut mono_frame = Vec::new();
        for _ in 0..FRAME_SIZE {
            mono_frame.extend_from_slice(&500_i16.to_le_bytes());
        }

        let output = denoiser.process(&mono_frame);

        assert_eq!(output.len(), FRAME_SIZE * BYTES_PER_SAMPLE);
    }

    #[test]
    fn steady_tone_keeps_its_length_across_many_chunks() {
        let mut denoiser = StreamDenoiser::new(CHANNEL_COUNT);
        let chunk = stereo_chunk(FRAME_SIZE, 3_000);

        // Ten frames in should give ten frames out once primed.
        let mut total_out = 0;
        for _ in 0..10 {
            total_out += denoiser.process(&chunk).len();
        }

        assert_eq!(total_out, 10 * FRAME_SIZE * 2 * BYTES_PER_SAMPLE);
    }
}
