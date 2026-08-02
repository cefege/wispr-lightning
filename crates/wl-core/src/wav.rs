//! Minimal RIFF/WAVE writer for 16 kHz mono s16le.
//!
//! Two consumers: the "Save recording" escape hatch in the retry UI, and the
//! Deepgram batch request body. A self-describing WAV container removes any
//! ambiguity about how the server should interpret headerless PCM.

use crate::consts::{CHANNELS, SAMPLE_RATE};

/// Size of a canonical 16-bit PCM WAV header.
pub const HEADER_LEN: usize = 44;

/// Build a WAV file from raw little-endian PCM bytes.
pub fn wrap_pcm(pcm: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + pcm.len());
    write_header(&mut out, pcm.len() as u32);
    out.extend_from_slice(pcm);
    out
}

/// Write a 44-byte header describing `data_len` bytes of s16le PCM.
pub fn write_header(out: &mut Vec<u8>, data_len: u32) {
    let bits_per_sample: u16 = 16;
    let block_align = CHANNELS * bits_per_sample / 8;
    let byte_rate = SAMPLE_RATE * u32::from(block_align);

    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");

    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // format = PCM
    out.extend_from_slice(&CHANNELS.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());

    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_is_exactly_44_bytes() {
        let mut out = Vec::new();
        write_header(&mut out, 0);
        assert_eq!(out.len(), HEADER_LEN);
    }

    #[test]
    fn riff_and_data_sizes_describe_the_payload() {
        let pcm = vec![0u8; 1280];
        let wav = wrap_pcm(&pcm);
        assert_eq!(wav.len(), HEADER_LEN + 1280);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()), 36 + 1280);
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 1280);
    }

    #[test]
    fn fmt_chunk_declares_16k_mono_s16le() {
        let wav = wrap_pcm(&[]);
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(u32::from_le_bytes(wav[16..20].try_into().unwrap()), 16);
        assert_eq!(u16::from_le_bytes(wav[20..22].try_into().unwrap()), 1); // PCM
        assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), 1); // mono
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
        assert_eq!(u32::from_le_bytes(wav[28..32].try_into().unwrap()), 32_000);
        assert_eq!(u16::from_le_bytes(wav[32..34].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(wav[34..36].try_into().unwrap()), 16);
    }

    #[test]
    fn payload_is_copied_verbatim() {
        let pcm: Vec<u8> = (0..=255u8).collect();
        assert_eq!(&wrap_pcm(&pcm)[HEADER_LEN..], &pcm[..]);
    }
}
