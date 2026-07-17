//! Transcribing a captured utterance with the OpenAI speech-to-text API.

use reqwest::multipart::{Form, Part};

use super::{error_for_status, OpenAiClient, OpenAiError};
use crate::audio::utterance::{CHANNEL_COUNT, SAMPLE_RATE_HZ};

const STT_MODEL: &str = "gpt-4o-transcribe";

const BITS_PER_SAMPLE: u16 = 16;
const BYTES_PER_SAMPLE: u32 = BITS_PER_SAMPLE as u32 / 8;
const WAV_HEADER_LENGTH: u32 = 44;

/// Wraps raw PCM in a WAV container.
///
/// The capture is headerless PCM, but the transcription API needs a recognisable audio
/// file, and a WAV header is 44 bytes of description in front of the exact same samples.
pub fn encode_pcm_as_wav(pcm: &[u8], sample_rate_hz: u32, channel_count: u16) -> Vec<u8> {
    let channel_count_u32 = u32::from(channel_count);
    let byte_rate = sample_rate_hz * channel_count_u32 * BYTES_PER_SAMPLE;
    let block_align = channel_count * BITS_PER_SAMPLE / 8;
    let pcm_length = pcm.len() as u32;

    let mut wav = Vec::with_capacity(WAV_HEADER_LENGTH as usize + pcm.len());

    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(WAV_HEADER_LENGTH - 8 + pcm_length).to_le_bytes());
    wav.extend_from_slice(b"WAVE");

    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes()); // fmt chunk length
    wav.extend_from_slice(&1_u16.to_le_bytes()); // format: uncompressed PCM
    wav.extend_from_slice(&channel_count.to_le_bytes());
    wav.extend_from_slice(&sample_rate_hz.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());

    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&pcm_length.to_le_bytes());
    wav.extend_from_slice(pcm);

    wav
}

impl OpenAiClient {
    /// Transcribes a single spoken utterance. Returns the text with surrounding
    /// whitespace removed, empty when nothing intelligible was said.
    pub async fn transcribe_speech(&self, utterance_pcm: &[u8]) -> Result<String, OpenAiError> {
        let wav = encode_pcm_as_wav(utterance_pcm, SAMPLE_RATE_HZ, CHANNEL_COUNT);

        let audio_part = Part::bytes(wav)
            .file_name("utterance.wav")
            .mime_str("audio/wav")
            // The MIME type is a fixed literal, so this cannot fail at runtime.
            .map_err(OpenAiError::Transport)?;

        let form = Form::new()
            .text("model", STT_MODEL)
            .part("file", audio_part);

        let response = self
            .http_client
            .post(self.endpoint("/audio/transcriptions"))
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await
            .map_err(OpenAiError::Transport)?;

        let transcription: TranscriptionResponse = error_for_status(response)
            .await?
            .json()
            .await
            .map_err(OpenAiError::Transport)?;

        Ok(transcription.text.trim().to_string())
    }
}

#[derive(Debug, serde::Deserialize)]
struct TranscriptionResponse {
    text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_u32(wav: &[u8], offset: usize) -> u32 {
        let bytes: [u8; 4] = wav[offset..offset + 4]
            .try_into()
            .expect("the slice is exactly four bytes");
        u32::from_le_bytes(bytes)
    }

    fn read_u16(wav: &[u8], offset: usize) -> u16 {
        let bytes: [u8; 2] = wav[offset..offset + 2]
            .try_into()
            .expect("the slice is exactly two bytes");
        u16::from_le_bytes(bytes)
    }

    #[test]
    fn the_header_is_the_documented_length_and_the_samples_follow_it() {
        let pcm = vec![7_u8; 400];

        let wav = encode_pcm_as_wav(&pcm, SAMPLE_RATE_HZ, CHANNEL_COUNT);

        assert_eq!(wav.len(), WAV_HEADER_LENGTH as usize + pcm.len());
        assert_eq!(&wav[WAV_HEADER_LENGTH as usize..], &pcm[..]);
    }

    #[test]
    fn the_riff_and_wave_markers_are_where_a_decoder_looks_for_them() {
        let wav = encode_pcm_as_wav(&[0; 8], SAMPLE_RATE_HZ, CHANNEL_COUNT);

        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
    }

    #[test]
    fn the_header_describes_the_capture_format() {
        let pcm = vec![0_u8; 960];

        let wav = encode_pcm_as_wav(&pcm, SAMPLE_RATE_HZ, CHANNEL_COUNT);

        assert_eq!(read_u16(&wav, 20), 1, "uncompressed PCM");
        assert_eq!(read_u16(&wav, 22), CHANNEL_COUNT);
        assert_eq!(read_u32(&wav, 24), SAMPLE_RATE_HZ);
        assert_eq!(read_u16(&wav, 34), BITS_PER_SAMPLE);
    }

    #[test]
    fn the_declared_sizes_match_the_actual_audio() {
        let pcm = vec![0_u8; 1_000];

        let wav = encode_pcm_as_wav(&pcm, SAMPLE_RATE_HZ, CHANNEL_COUNT);

        // RIFF size counts everything after the first eight bytes.
        assert_eq!(read_u32(&wav, 4), WAV_HEADER_LENGTH - 8 + pcm.len() as u32);
        assert_eq!(read_u32(&wav, 40), pcm.len() as u32);
    }

    #[test]
    fn byte_rate_and_block_align_follow_from_the_format() {
        let wav = encode_pcm_as_wav(&[0; 4], 48_000, 2);

        // 48000 samples/s * 2 channels * 2 bytes = 192000 bytes/s
        assert_eq!(read_u32(&wav, 28), 192_000);
        // 2 channels * 2 bytes per sample
        assert_eq!(read_u16(&wav, 32), 4);
    }

    #[test]
    fn an_empty_utterance_still_produces_a_valid_header() {
        let wav = encode_pcm_as_wav(&[], SAMPLE_RATE_HZ, CHANNEL_COUNT);

        assert_eq!(wav.len(), WAV_HEADER_LENGTH as usize);
        assert_eq!(read_u32(&wav, 40), 0);
    }
}
