//! Audio framing: turning a stream of 16 kHz mono `i16` samples into the
//! fixed-size packets the transcription protocol expects, and computing the
//! per-packet RMS the backend wants alongside them.

use crate::consts::{CHUNK_BYTES, CHUNK_SAMPLES};

/// Accumulates `i16` samples and emits fixed 640-sample packets.
///
/// The Swift implementation discarded the sub-640-sample tail of *every*
/// converted buffer, silently losing up to 40 ms per audio callback. This
/// carries the remainder across calls instead — see `PORT_PLAN.md` DV1.
#[derive(Debug, Default)]
pub struct Packetizer {
    carry: Vec<i16>,
}

impl Packetizer {
    pub fn new() -> Self {
        Self {
            carry: Vec::with_capacity(CHUNK_SAMPLES),
        }
    }

    /// Feed samples, invoking `emit` once per complete packet.
    pub fn push(&mut self, samples: &[i16], mut emit: impl FnMut(&[i16])) {
        let mut rest = samples;

        if !self.carry.is_empty() {
            let need = CHUNK_SAMPLES - self.carry.len();
            let take = need.min(rest.len());
            self.carry.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
            if self.carry.len() < CHUNK_SAMPLES {
                return;
            }
            emit(&self.carry);
            self.carry.clear();
        }

        let full = rest.len() / CHUNK_SAMPLES * CHUNK_SAMPLES;
        for packet in rest[..full].chunks_exact(CHUNK_SAMPLES) {
            emit(packet);
        }
        self.carry.extend_from_slice(&rest[full..]);
    }

    /// Zero-pad and emit any buffered remainder. Called once at end of
    /// recording so the final partial frame is not lost.
    pub fn flush(&mut self, mut emit: impl FnMut(&[i16])) {
        if self.carry.is_empty() {
            return;
        }
        self.carry.resize(CHUNK_SAMPLES, 0);
        emit(&self.carry);
        self.carry.clear();
    }

    /// Samples currently buffered and not yet emitted.
    pub fn pending(&self) -> usize {
        self.carry.len()
    }
}

/// Convert one packet of samples to little-endian bytes.
pub fn packet_to_le_bytes(samples: &[i16]) -> Vec<u8> {
    debug_assert_eq!(samples.len(), CHUNK_SAMPLES);
    let mut out = Vec::with_capacity(CHUNK_BYTES);
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Per-packet RMS volume as the backend expects it.
///
/// `round(rms / 32768 * 10000) / 10000` — normalized to roughly `0.0..=0.3052`
/// and rounded to four decimal places, half away from zero (matching Swift's
/// `Double.rounded()`).
pub fn packet_volume(samples: &[i16]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_squares: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    let rms = (sum_squares / samples.len() as f64).sqrt();
    (rms / 32768.0 * 10000.0).round() / 10000.0
}

/// Downmix an interleaved multi-channel `f32` frame buffer to mono by
/// averaging. Taking channel 0 is wrong: on a stereo interface with the mic on
/// the right input, channel 0 is silence.
pub fn downmix_to_mono(interleaved: &[f32], channels: usize, out: &mut Vec<f32>) {
    out.clear();
    if channels <= 1 {
        out.extend_from_slice(interleaved);
        return;
    }
    let inv = 1.0 / channels as f32;
    out.extend(
        interleaved
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() * inv),
    );
}

/// Quantize normalized `f32` samples to `i16`, clamping to avoid wrap-around on
/// overdriven input.
pub fn quantize(input: &[f32], out: &mut Vec<i16>) {
    out.clear();
    out.extend(
        input
            .iter()
            .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(p: &mut Packetizer, samples: &[i16]) -> Vec<Vec<i16>> {
        let mut got = Vec::new();
        p.push(samples, |pkt| got.push(pkt.to_vec()));
        got
    }

    #[test]
    fn exact_multiple_emits_whole_packets_and_buffers_nothing() {
        let mut p = Packetizer::new();
        let got = collect(&mut p, &vec![7i16; CHUNK_SAMPLES * 3]);
        assert_eq!(got.len(), 3);
        assert!(got.iter().all(|pkt| pkt.len() == CHUNK_SAMPLES));
        assert_eq!(p.pending(), 0);
    }

    #[test]
    fn remainder_carries_across_calls_instead_of_being_dropped() {
        let mut p = Packetizer::new();
        // Two callbacks of 400 samples: neither alone fills a packet, together
        // they exceed one. The Swift version emitted nothing here.
        assert!(collect(&mut p, &vec![1i16; 400]).is_empty());
        assert_eq!(p.pending(), 400);

        let got = collect(&mut p, &vec![2i16; 400]);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0][..400], vec![1i16; 400][..]);
        assert_eq!(got[0][400..], vec![2i16; CHUNK_SAMPLES - 400][..]);
        assert_eq!(p.pending(), 800 - CHUNK_SAMPLES);
    }

    #[test]
    fn no_sample_is_lost_across_a_ragged_stream() {
        let mut p = Packetizer::new();
        let mut emitted = 0usize;
        // Buffer sizes that share no common factor with 640.
        for size in [37, 1021, 3, 512, 999, 7, 4096] {
            p.push(&vec![0i16; size], |pkt| emitted += pkt.len());
        }
        let total: usize = [37, 1021, 3, 512, 999, 7, 4096].iter().sum();
        assert_eq!(emitted + p.pending(), total);
    }

    #[test]
    fn flush_zero_pads_the_final_partial_packet() {
        let mut p = Packetizer::new();
        collect(&mut p, &[5i16; 100]);
        let mut got = Vec::new();
        p.flush(|pkt| got.push(pkt.to_vec()));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].len(), CHUNK_SAMPLES);
        assert_eq!(got[0][99], 5);
        assert_eq!(got[0][100], 0);
        assert_eq!(p.pending(), 0);
    }

    #[test]
    fn flush_on_empty_buffer_emits_nothing() {
        let mut got = 0;
        Packetizer::new().flush(|_| got += 1);
        assert_eq!(got, 0);
    }

    #[test]
    fn silence_has_zero_volume_and_full_scale_normalizes_to_one() {
        assert_eq!(packet_volume(&[0; CHUNK_SAMPLES]), 0.0);
        // rms 32767 / 32768 * 10000 rounds to 10000, i.e. 1.0.
        assert_eq!(packet_volume(&[i16::MAX; CHUNK_SAMPLES]), 1.0);
        assert_eq!(packet_volume(&[16384i16; CHUNK_SAMPLES]), 0.5);
    }

    #[test]
    fn volume_is_rounded_to_four_decimals() {
        let v = packet_volume(&[1234i16; CHUNK_SAMPLES]);
        assert_eq!(v, (v * 10000.0).round() / 10000.0);
        assert!((0.0..=1.0).contains(&v));
    }

    #[test]
    fn an_empty_packet_has_no_volume_rather_than_nan() {
        assert_eq!(packet_volume(&[]), 0.0);
    }

    #[test]
    fn packet_bytes_are_little_endian_and_correctly_sized() {
        let bytes = packet_to_le_bytes(&[0x0102i16; CHUNK_SAMPLES]);
        assert_eq!(bytes.len(), CHUNK_BYTES);
        assert_eq!(&bytes[..2], &[0x02, 0x01]);
    }

    #[test]
    fn downmix_averages_channels_rather_than_taking_the_first() {
        let mut out = Vec::new();
        // Left silent, right carrying the signal — the common USB-interface case.
        downmix_to_mono(&[0.0, 1.0, 0.0, 1.0], 2, &mut out);
        assert_eq!(out, vec![0.5, 0.5]);
    }

    #[test]
    fn downmix_passes_mono_through_untouched() {
        let mut out = Vec::new();
        downmix_to_mono(&[0.25, -0.5], 1, &mut out);
        assert_eq!(out, vec![0.25, -0.5]);
    }

    #[test]
    fn quantize_clamps_instead_of_wrapping() {
        let mut out = Vec::new();
        quantize(&[2.0, -2.0, 0.0], &mut out);
        assert_eq!(out, vec![i16::MAX, -i16::MAX, 0]);
    }
}
