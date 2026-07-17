//! Capturing the microphone and handing the audio over in the pipeline's format.
//!
//! A device records in whatever format it likes — 44.1kHz mono float is as common as
//! 48kHz stereo — so everything captured is converted to the one format the rest of the
//! pipeline and Discord expect: 48kHz stereo signed 16-bit PCM.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
// Sample carries the from_sample conversion; FromSample is the bound that allows it.
use cpal::{FromSample, Sample};
use tokio::sync::mpsc::UnboundedSender;

use super::utterance::{CHANNEL_COUNT, SAMPLE_RATE_HZ};

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("no microphone is available")]
    NoDefaultDevice,

    #[error("the microphone \"{0}\" is not connected")]
    DeviceNotFound(String),

    #[error("could not list the microphones: {0}")]
    ListDevices(#[source] cpal::DevicesError),

    #[error("could not read the microphone's recording format: {0}")]
    DefaultConfig(#[source] cpal::DefaultStreamConfigError),

    #[error("this microphone records in {0:?}, which is not supported")]
    UnsupportedSampleFormat(cpal::SampleFormat),

    #[error("could not open the microphone: {0}")]
    BuildStream(#[source] cpal::BuildStreamError),

    #[error("could not start the microphone: {0}")]
    Play(#[source] cpal::PlayStreamError),

    #[error("could not start the capture thread: {0}")]
    Thread(#[source] std::io::Error),

    #[error("the capture thread stopped before it could say whether the microphone opened")]
    ThreadGone,
}

/// Every input device the system offers, named as [`start_capture`] expects them.
///
/// A device whose name cannot be read is left out: it could not be selected by name
/// anyway, so listing it would only offer the user a choice that cannot work.
pub fn list_microphone_names() -> Result<Vec<String>, CaptureError> {
    let host = cpal::default_host();
    let devices = host.input_devices().map_err(CaptureError::ListDevices)?;
    let names = devices.filter_map(|device| device.name().ok()).collect();

    Ok(names)
}

/// The name of the device used when the settings do not name one.
pub fn default_microphone_name() -> Option<String> {
    let host = cpal::default_host();
    let device = host.default_input_device()?;

    device.name().ok()
}

/// A running capture. Dropping this closes the microphone, which is what turns the
/// recording light back off.
pub struct CaptureHandle {
    stop_sender: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl CaptureHandle {
    /// Closes the microphone and waits for the capture thread to finish.
    pub fn stop(&mut self) {
        // A closed channel means the thread has already gone, which is the state we
        // wanted anyway.
        if let Some(stop_sender) = self.stop_sender.take() {
            let _stop_delivered = stop_sender.send(());
        }

        let Some(thread) = self.thread.take() else {
            return;
        };

        if thread.join().is_err() {
            tracing::warn!("the microphone capture thread panicked on the way out");
        }
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Opens a microphone and streams it as 48kHz stereo 16-bit PCM into `pcm_sender`.
///
/// Pass an empty `microphone_name` to use the system default device.
pub fn start_capture(
    microphone_name: &str,
    pcm_sender: UnboundedSender<Vec<u8>>,
) -> Result<CaptureHandle, CaptureError> {
    let (startup_sender, startup_receiver) = std::sync::mpsc::channel();
    let (stop_sender, stop_receiver) = std::sync::mpsc::channel();
    let requested_name = microphone_name.to_string();

    // cpal's Stream is not Send on Windows, so it has to be built, played and dropped on
    // one thread. That thread parks until asked to stop.
    let thread = std::thread::Builder::new()
        .name("microphone-capture".to_string())
        .spawn(move || {
            run_capture_thread(&requested_name, pcm_sender, &startup_sender, &stop_receiver);
        })
        .map_err(CaptureError::Thread)?;

    match startup_receiver.recv() {
        Ok(Ok(())) => Ok(CaptureHandle {
            stop_sender: Some(stop_sender),
            thread: Some(thread),
        }),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(CaptureError::ThreadGone),
    }
}

/// Owns the stream for its whole life: opens the device, reports whether that worked,
/// then waits to be told to stop.
fn run_capture_thread(
    microphone_name: &str,
    pcm_sender: UnboundedSender<Vec<u8>>,
    startup_sender: &std::sync::mpsc::Sender<Result<(), CaptureError>>,
    stop_receiver: &std::sync::mpsc::Receiver<()>,
) {
    let stream = match open_stream(microphone_name, pcm_sender) {
        Ok(stream) => stream,
        Err(error) => {
            report_startup(startup_sender, Err(error));
            return;
        }
    };

    if let Err(error) = stream.play() {
        report_startup(startup_sender, Err(CaptureError::Play(error)));
        return;
    }

    report_startup(startup_sender, Ok(()));

    // Blocks until stop() is called or the handle is dropped. Both mean the same thing:
    // fall through, drop the stream, and let go of the microphone.
    let _stop_signal = stop_receiver.recv();
}

fn report_startup(
    startup_sender: &std::sync::mpsc::Sender<Result<(), CaptureError>>,
    outcome: Result<(), CaptureError>,
) {
    if startup_sender.send(outcome).is_err() {
        tracing::warn!("the microphone opened but nobody was waiting to hear about it");
    }
}

/// Finds the device, reads the format it wants to record in, and opens a stream that
/// converts each chunk into the pipeline's format.
fn open_stream(
    microphone_name: &str,
    pcm_sender: UnboundedSender<Vec<u8>>,
) -> Result<cpal::Stream, CaptureError> {
    let device = find_device(microphone_name)?;
    // The device's own default is the format it is already set up for, so it always
    // opens. Anything else risks being refused, and converting is cheap.
    let supported_config = device
        .default_input_config()
        .map_err(CaptureError::DefaultConfig)?;

    let source_format = SourceFormat {
        sample_rate_hz: supported_config.sample_rate().0,
        channel_count: supported_config.channels(),
    };
    let sample_format = supported_config.sample_format();
    let config = supported_config.into();

    tracing::info!(
        microphone = %device.name().unwrap_or_else(|_| "unknown".to_string()),
        sample_rate_hz = source_format.sample_rate_hz,
        channels = source_format.channel_count,
        ?sample_format,
        "opening microphone"
    );

    match sample_format {
        cpal::SampleFormat::F32 => build_stream::<f32>(&device, &config, source_format, pcm_sender),
        cpal::SampleFormat::I16 => build_stream::<i16>(&device, &config, source_format, pcm_sender),
        cpal::SampleFormat::U16 => build_stream::<u16>(&device, &config, source_format, pcm_sender),
        unsupported => Err(CaptureError::UnsupportedSampleFormat(unsupported)),
    }
}

fn find_device(microphone_name: &str) -> Result<cpal::Device, CaptureError> {
    let host = cpal::default_host();

    if microphone_name.trim().is_empty() {
        return host
            .default_input_device()
            .ok_or(CaptureError::NoDefaultDevice);
    }

    let mut devices = host.input_devices().map_err(CaptureError::ListDevices)?;
    let matching_device =
        devices.find(|device| device.name().is_ok_and(|name| name == microphone_name));

    matching_device.ok_or_else(|| CaptureError::DeviceNotFound(microphone_name.to_string()))
}

/// Builds the stream for one sample format. Generic so the three formats cpal reports
/// share this code rather than repeating it.
fn build_stream<SourceSample>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    source_format: SourceFormat,
    pcm_sender: UnboundedSender<Vec<u8>>,
) -> Result<cpal::Stream, CaptureError>
where
    SourceSample: cpal::SizedSample,
    i16: FromSample<SourceSample>,
{
    let stream = device
        .build_input_stream(
            config,
            move |captured: &[SourceSample], _info: &cpal::InputCallbackInfo| {
                let samples: Vec<i16> = captured
                    .iter()
                    .map(|sample| i16::from_sample(*sample))
                    .collect();
                let pcm = convert_to_pipeline_pcm(&samples, source_format);

                // A closed channel means the session has already stopped and simply has
                // not got round to closing the microphone yet. Nothing to do about it
                // from an audio callback.
                let _receiver_still_listening = pcm_sender.send(pcm);
            },
            |error| {
                tracing::error!("microphone capture error: {error}");
            },
            None,
        )
        .map_err(CaptureError::BuildStream)?;

    Ok(stream)
}

/// The format a device hands us audio in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceFormat {
    sample_rate_hz: u32,
    channel_count: u16,
}

/// Converts a chunk of captured samples into 48kHz stereo 16-bit PCM bytes.
fn convert_to_pipeline_pcm(samples: &[i16], source_format: SourceFormat) -> Vec<u8> {
    let stereo_frames = to_stereo_frames(samples, source_format.channel_count);
    let resampled_frames = resample_frames(&stereo_frames, source_format.sample_rate_hz);

    frames_to_bytes(&resampled_frames)
}

/// Lays interleaved samples out as stereo frames: a mono device is doubled onto both
/// ears, and a device with more channels than we need is cut down to the first two.
fn to_stereo_frames(samples: &[i16], source_channel_count: u16) -> Vec<[i16; 2]> {
    let channel_count = usize::from(source_channel_count.max(1));

    samples
        .chunks_exact(channel_count)
        .map(|frame| match frame {
            [only_channel] => [*only_channel, *only_channel],
            [left, right, ..] => [*left, *right],
            // chunks_exact never yields a short chunk, so this is unreachable in
            // practice; silence is the harmless answer if it ever were.
            _ => [0, 0],
        })
        .collect()
}

/// Resamples frames to 48kHz by interpolating between neighbours.
///
/// Each chunk is resampled on its own, so a chunk boundary can land mid-interval and
/// lose a fraction of a sample. At the sizes involved that is far below anything audible
/// or anything transcription would notice, and it avoids carrying resampler state across
/// callbacks.
fn resample_frames(frames: &[[i16; 2]], source_rate_hz: u32) -> Vec<[i16; 2]> {
    if source_rate_hz == SAMPLE_RATE_HZ || frames.is_empty() || source_rate_hz == 0 {
        return frames.to_vec();
    }

    let source_rate = f64::from(source_rate_hz);
    let target_rate = f64::from(SAMPLE_RATE_HZ);
    let output_frame_count = ((frames.len() as f64) * target_rate / source_rate).round() as usize;
    let last_index = frames.len() - 1;

    let mut resampled = Vec::with_capacity(output_frame_count);

    for output_index in 0..output_frame_count {
        let source_position = (output_index as f64) * source_rate / target_rate;
        let left_index = (source_position.floor() as usize).min(last_index);
        let right_index = (left_index + 1).min(last_index);
        let fraction = source_position - source_position.floor();

        resampled.push(interpolate_frame(
            frames[left_index],
            frames[right_index],
            fraction,
        ));
    }

    resampled
}

fn interpolate_frame(earlier: [i16; 2], later: [i16; 2], fraction: f64) -> [i16; 2] {
    [
        interpolate_sample(earlier[0], later[0], fraction),
        interpolate_sample(earlier[1], later[1], fraction),
    ]
}

fn interpolate_sample(earlier: i16, later: i16, fraction: f64) -> i16 {
    let earlier_value = f64::from(earlier);
    let later_value = f64::from(later);
    let interpolated = earlier_value + (later_value - earlier_value) * fraction;

    // Clamped to the i16 range first, so the cast cannot truncate.
    let clamped = interpolated
        .round()
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX));

    clamped as i16
}

fn frames_to_bytes(frames: &[[i16; 2]]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(frames.len() * usize::from(CHANNEL_COUNT) * 2);

    for frame in frames {
        bytes.extend_from_slice(&frame[0].to_le_bytes());
        bytes.extend_from_slice(&frame[1].to_le_bytes());
    }

    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mono_device_is_doubled_onto_both_ears() {
        let mono_samples = [100_i16, 200, 300];

        let frames = to_stereo_frames(&mono_samples, 1);

        assert_eq!(frames, vec![[100, 100], [200, 200], [300, 300]]);
    }

    #[test]
    fn a_stereo_device_is_taken_as_it_comes() {
        let stereo_samples = [100_i16, -100, 200, -200];

        let frames = to_stereo_frames(&stereo_samples, 2);

        assert_eq!(frames, vec![[100, -100], [200, -200]]);
    }

    #[test]
    fn a_surround_device_is_cut_down_to_the_first_two_channels() {
        // Six interleaved channels, one frame's worth.
        let surround_samples = [10_i16, 20, 30, 40, 50, 60];

        let frames = to_stereo_frames(&surround_samples, 6);

        assert_eq!(frames, vec![[10, 20]]);
    }

    #[test]
    fn a_partial_frame_at_the_end_is_dropped_rather_than_faked() {
        // Five samples from a stereo device: the last one has no pair.
        let samples = [1_i16, 2, 3, 4, 5];

        let frames = to_stereo_frames(&samples, 2);

        assert_eq!(frames, vec![[1, 2], [3, 4]]);
    }

    #[test]
    fn audio_already_at_the_pipeline_rate_is_left_alone() {
        let frames = vec![[1_i16, 2], [3, 4], [5, 6]];

        let resampled = resample_frames(&frames, SAMPLE_RATE_HZ);

        assert_eq!(resampled, frames);
    }

    #[test]
    fn doubling_the_rate_doubles_the_frame_count() {
        // 24kHz is what OpenAI's raw PCM comes back at, and half of what Discord wants.
        let frames = vec![[0_i16, 0]; 240];

        let resampled = resample_frames(&frames, 24_000);

        assert_eq!(resampled.len(), 480);
    }

    #[test]
    fn the_common_forty_four_one_case_lands_on_the_right_length() {
        // One second of 44.1kHz should come out as one second of 48kHz.
        let frames = vec![[0_i16, 0]; 44_100];

        let resampled = resample_frames(&frames, 44_100);

        assert_eq!(resampled.len(), 48_000);
    }

    #[test]
    fn interpolation_lands_between_the_neighbours() {
        assert_eq!(interpolate_sample(0, 100, 0.5), 50);
        assert_eq!(interpolate_sample(0, 100, 0.0), 0);
        assert_eq!(interpolate_sample(-100, 100, 0.5), 0);
    }

    #[test]
    fn interpolation_cannot_overflow_the_sample_range() {
        assert_eq!(interpolate_sample(i16::MAX, i16::MAX, 0.5), i16::MAX);
        assert_eq!(interpolate_sample(i16::MIN, i16::MIN, 0.5), i16::MIN);
    }

    #[test]
    fn resampling_preserves_a_steady_signal() {
        let frames = vec![[1_000_i16, -1_000]; 100];

        let resampled = resample_frames(&frames, 44_100);

        // Interpolating between identical neighbours must not invent a wobble.
        assert!(resampled.iter().all(|frame| *frame == [1_000_i16, -1_000]));
    }

    #[test]
    fn frames_become_little_endian_pairs() {
        let frames = [[1_i16, -1]];

        let bytes = frames_to_bytes(&frames);

        assert_eq!(bytes, vec![0x01, 0x00, 0xFF, 0xFF]);
    }

    #[test]
    fn an_empty_chunk_converts_to_nothing_rather_than_failing() {
        let source_format = SourceFormat {
            sample_rate_hz: 44_100,
            channel_count: 2,
        };

        assert!(convert_to_pipeline_pcm(&[], source_format).is_empty());
    }

    #[test]
    fn a_mono_forty_four_one_chunk_comes_out_as_stereo_forty_eight_k() {
        let source_format = SourceFormat {
            sample_rate_hz: 44_100,
            channel_count: 1,
        };
        // 100ms of mono 44.1kHz audio.
        let samples = vec![500_i16; 4_410];

        let pcm = convert_to_pipeline_pcm(&samples, source_format);

        // 100ms of stereo 48kHz PCM: 4800 frames * 2 channels * 2 bytes.
        assert_eq!(pcm.len(), 4_800 * 2 * 2);
    }
}
