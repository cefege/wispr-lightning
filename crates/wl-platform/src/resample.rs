//! Turning whatever the microphone hands us into the packets the protocol
//! wants: 16 kHz mono `i16` in fixed 640-sample frames.
//!
//! Hardware gives 44.1 or 48 kHz `f32` with one to eight channels; the wire
//! format is fixed. The order of operations is deliberate — **downmix, then
//! resample** — so the resampler runs over one channel instead of N. Doing it
//! the other way round costs a multiple of the CPU for an identical result.
//!
//! Nothing here touches cpal or a thread, which is the point: the whole
//! conversion is exercised by unit tests over synthetic input, and only the
//! plumbing in [`crate::audio_impl`] needs a real microphone.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Indexing, Resampler};
use wl_core::audio::{downmix_to_mono, quantize, Packetizer};
use wl_core::consts::{CHUNK_SAMPLES, SAMPLE_RATE};

use crate::{PlatformError, Result};

/// How much all-zero input must accumulate before it counts as "the device is
/// not actually giving us audio". Short recordings say nothing useful: a
/// 200 ms take can legitimately be pure zeros if the user misfired the hotkey.
const SILENCE_WINDOW_MS: usize = 500;

/// Watches for a capture that is digital silence rather than quiet.
///
/// A microphone blocked by Windows' global privacy toggle frequently produces
/// a stream of exact zeros instead of an error (see `research-audio-deepgram`
/// Q4), and that is indistinguishable from a working microphone in a quiet
/// room *unless* you look at the whole take: real hardware has a noise floor,
/// so even a silent room yields non-zero samples within milliseconds.
///
/// Hence the two conditions: at least [`SilenceGuard::min_frames`] observed,
/// and not one non-zero sample in the entire window.
#[derive(Debug, Clone)]
pub struct SilenceGuard {
    min_frames: usize,
    frames: usize,
    saw_signal: bool,
}

impl SilenceGuard {
    /// `min_frames` is measured in mono frames at the *input* sample rate.
    pub fn new(min_frames: usize) -> Self {
        Self {
            min_frames,
            frames: 0,
            saw_signal: false,
        }
    }

    /// Minimum number of frames before a verdict is possible.
    pub fn min_frames(&self) -> usize {
        self.min_frames
    }

    pub fn observe(&mut self, samples: &[f32]) {
        self.frames += samples.len();
        // Short-circuit once a signal is seen: from then on the scan is dead
        // weight on every buffer for the rest of the recording.
        if !self.saw_signal {
            self.saw_signal = samples.iter().any(|&s| s != 0.0);
        }
    }

    /// Whether the capture so far is long enough to judge and entirely zero.
    pub fn is_silent(&self) -> bool {
        self.frames >= self.min_frames && !self.saw_signal
    }

    pub fn reset(&mut self) {
        self.frames = 0;
        self.saw_signal = false;
    }
}

/// The downmix → resample → quantize → packetize pipeline for one recording.
///
/// Construct once per stream configuration and reuse across recordings via
/// [`Converter::reset`]; construction allocates and builds FFT twiddle tables,
/// so it must not happen while the user is waiting to speak.
pub struct Converter {
    channels: usize,
    /// `None` when the device already runs at 16 kHz, in which case samples
    /// pass through untouched. Rare — macOS built-in microphones only offer
    /// 48 kHz — but free when it happens.
    resampler: Option<Fft<f32>>,
    /// Downmix scratch, one entry per input frame of the current buffer.
    mono: Vec<f32>,
    /// Mono frames accepted but not yet consumed by the resampler. With
    /// `FixedSync::Output` the resampler's appetite varies call to call and has
    /// nothing to do with the size of a cpal buffer, so the two must be
    /// decoupled by a queue.
    pending: Vec<f32>,
    resampled: Vec<f32>,
    pcm: Vec<i16>,
    packetizer: Packetizer,
    guard: SilenceGuard,
}

impl Converter {
    pub fn new(input_rate: u32, channels: u16) -> Result<Self> {
        if input_rate == 0 {
            return Err(PlatformError::AudioDevice(
                "device reported a sample rate of zero".into(),
            ));
        }
        let channels = usize::from(channels).max(1);

        let resampler = if input_rate == SAMPLE_RATE {
            None
        } else {
            // `FixedSync::Output` pins the *output* chunk at exactly one
            // packet, so every successful call yields precisely the 640 frames
            // the protocol wants and no partial-packet bookkeeping is needed
            // between the resampler and the packetizer.
            let fft = Fft::<f32>::new(
                input_rate as usize,
                SAMPLE_RATE as usize,
                CHUNK_SAMPLES,
                1,
                FixedSync::Output,
            )
            .map_err(|e| {
                PlatformError::AudioDevice(format!(
                    "cannot resample {input_rate} Hz to {SAMPLE_RATE} Hz: {e}"
                ))
            })?;
            Some(fft)
        };

        let (max_in, max_out) = match resampler.as_ref() {
            Some(rs) => (rs.input_frames_max(), rs.output_frames_max()),
            None => (0, 0),
        };

        Ok(Self {
            channels,
            resampler,
            mono: Vec::new(),
            // Room for one full demand plus a large driver buffer on top, so
            // the queue never reallocates mid-recording.
            pending: Vec::with_capacity(max_in * 2),
            resampled: vec![0.0; max_out],
            pcm: Vec::with_capacity(max_out.max(CHUNK_SAMPLES)),
            packetizer: Packetizer::new(),
            guard: SilenceGuard::new(input_rate as usize * SILENCE_WINDOW_MS / 1000),
        })
    }

    /// Whether a resampler is in the path. `false` means the device is already
    /// at 16 kHz and samples are only downmixed and quantized.
    pub fn resamples(&self) -> bool {
        self.resampler.is_some()
    }

    /// See [`SilenceGuard::is_silent`].
    pub fn input_was_silent(&self) -> bool {
        self.guard.is_silent()
    }

    /// Feed one interleaved buffer straight from the capture callback, calling
    /// `emit` once per complete 640-sample packet.
    pub fn push_interleaved(&mut self, data: &[f32], mut emit: impl FnMut(&[i16])) -> Result<()> {
        let Self {
            channels,
            resampler,
            mono,
            pending,
            resampled,
            pcm,
            packetizer,
            guard,
        } = self;

        downmix_to_mono(data, *channels, mono);
        guard.observe(mono);

        let Some(rs) = resampler.as_mut() else {
            quantize(mono, pcm);
            packetizer.push(pcm, &mut emit);
            return Ok(());
        };

        pending.extend_from_slice(mono);

        let mut consumed = 0;
        loop {
            let want = rs.input_frames_next();
            if pending.len() - consumed < want {
                break;
            }
            let (used, produced) = run(
                rs,
                &pending[consumed..consumed + want],
                want,
                None,
                resampled,
            )?;
            consumed += used;
            quantize(&resampled[..produced], pcm);
            packetizer.push(pcm, &mut emit);
        }
        pending.drain(..consumed);
        Ok(())
    }

    /// Flush the tail of a recording: the frames still queued for the
    /// resampler, then the packetizer's partial packet.
    ///
    /// The final packet is zero-padded rather than dropped. The Swift original
    /// discarded every sub-640 remainder (`PORT_PLAN.md` DV1); losing the last
    /// word of a dictation is a user-visible bug, and the backend tolerates a
    /// few milliseconds of trailing silence.
    pub fn finish(&mut self, mut emit: impl FnMut(&[i16])) -> Result<()> {
        let Self {
            resampler,
            pending,
            resampled,
            pcm,
            packetizer,
            ..
        } = self;

        if let Some(rs) = resampler.as_mut() {
            if !pending.is_empty() {
                // `partial_len` tells the resampler how much of the buffer is
                // real; it substitutes silence for the rest, which is exactly
                // the end-of-stream semantics we want.
                let real = pending.len();
                let want = rs.input_frames_next().max(real);
                pending.resize(want, 0.0);
                let indexing = Indexing {
                    partial_len: Some(real),
                    ..Indexing::default()
                };
                let (_, produced) = run(rs, pending, want, Some(&indexing), resampled)?;
                quantize(&resampled[..produced], pcm);
                packetizer.push(pcm, &mut emit);
                pending.clear();
            }
        }

        packetizer.flush(&mut emit);
        Ok(())
    }

    /// Return to a clean state for the next recording, keeping the FFT tables.
    pub fn reset(&mut self) {
        if let Some(rs) = self.resampler.as_mut() {
            rs.reset();
        }
        self.pending.clear();
        self.packetizer = Packetizer::new();
        self.guard.reset();
    }
}

/// One `process_into_buffer` call, with the audioadapter wrapping kept in one
/// place. Returns `(input frames consumed, output frames produced)`.
fn run(
    rs: &mut Fft<f32>,
    input: &[f32],
    frames: usize,
    indexing: Option<&Indexing>,
    output: &mut [f32],
) -> Result<(usize, usize)> {
    let adapter_in = InterleavedSlice::new(input, 1, frames)
        .map_err(|e| PlatformError::AudioDevice(format!("resampler input buffer: {e}")))?;
    let out_frames = output.len();
    let mut adapter_out = InterleavedSlice::new_mut(output, 1, out_frames)
        .map_err(|e| PlatformError::AudioDevice(format!("resampler output buffer: {e}")))?;
    rs.process_into_buffer(&adapter_in, &mut adapter_out, indexing)
        .map_err(|e| PlatformError::AudioDevice(format!("resampling failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Interleaved sine, `frames` frames of `channels` channels. Every channel
    /// carries the same tone, so the average downmix is lossless and the
    /// resampled result is comparable against a single-channel run.
    fn sine(frames: usize, channels: usize, rate: u32, hz: f32) -> Vec<f32> {
        let mut out = Vec::with_capacity(frames * channels);
        for n in 0..frames {
            let v = (std::f32::consts::TAU * hz * n as f32 / rate as f32).sin() * 0.5;
            for _ in 0..channels {
                out.push(v);
            }
        }
        out
    }

    fn drain(
        conv: &mut Converter,
        data: &[f32],
        chunk_frames: &[usize],
        channels: usize,
    ) -> Vec<Vec<i16>> {
        let mut packets = Vec::new();
        let mut at = 0;
        let mut sizes = chunk_frames.iter().cycle();
        while at < data.len() {
            let take = (sizes.next().copied().unwrap_or(1) * channels).min(data.len() - at);
            conv.push_interleaved(&data[at..at + take], |p| packets.push(p.to_vec()))
                .unwrap();
            at += take;
        }
        packets
    }

    #[test]
    fn a_48k_stereo_second_becomes_exactly_one_second_of_16k_packets() {
        // 48 000 frames in, 3:1 decimation, 16 000 mono samples out,
        // 16 000 / 640 = 25 packets. Any off-by-one in the queue accounting
        // shows up here as a missing or extra packet.
        let data = sine(48_000, 2, 48_000, 440.0);
        let mut conv = Converter::new(48_000, 2).unwrap();
        assert!(conv.resamples());

        let mut packets = drain(&mut conv, &data, &[1024], 2);
        conv.finish(|p| packets.push(p.to_vec())).unwrap();

        assert_eq!(packets.len(), 25);
        assert!(packets.iter().all(|p| p.len() == CHUNK_SAMPLES));
    }

    #[test]
    fn ragged_capture_buffers_produce_the_identical_stream_as_one_big_buffer() {
        // cpal buffer sizes are the driver's business and change under load;
        // the packet stream must not depend on them.
        let data = sine(48_000, 2, 48_000, 440.0);

        let mut whole = Converter::new(48_000, 2).unwrap();
        let mut expected = drain(&mut whole, &data, &[48_000], 2);
        whole.finish(|p| expected.push(p.to_vec())).unwrap();

        // Deliberately co-prime with both 640 and the resampler's 1920-frame
        // appetite, and varying call to call.
        let mut ragged = Converter::new(48_000, 2).unwrap();
        let mut actual = drain(&mut ragged, &data, &[7, 191, 1023, 3, 4096], 2);
        ragged.finish(|p| actual.push(p.to_vec())).unwrap();

        assert_eq!(actual, expected);
    }

    #[test]
    fn the_frames_still_queued_for_the_resampler_are_not_lost_at_stop() {
        // The resampler only runs when it has 1920 frames to chew on, so at
        // the moment the user releases the key there are up to 40 ms of speech
        // sitting in the queue. Releasing mid-word must not eat it.
        let data = sine(49_000, 1, 48_000, 440.0);
        let mut conv = Converter::new(48_000, 1).unwrap();

        let mut packets = drain(&mut conv, &data, &[4096], 1);
        assert_eq!(packets.len(), 25, "48 000 of the 49 000 frames resampled");

        conv.finish(|p| packets.push(p.to_vec())).unwrap();
        assert_eq!(packets.len(), 26, "the queued 1 000 frames became a packet");
        assert!(packets.iter().all(|p| p.len() == CHUNK_SAMPLES));

        // 1 000 input frames at 3:1 is ~333 output samples of real audio.
        let tail = &packets[25];
        assert!(
            tail[..300].iter().any(|&s| s.abs() > 1_000),
            "the flushed packet is silence, so the tail was dropped"
        );
    }

    #[test]
    fn a_16k_mono_device_bypasses_the_resampler_and_is_bit_exact() {
        let data = sine(6_400, 1, 16_000, 440.0);
        let mut conv = Converter::new(16_000, 1).unwrap();
        assert!(!conv.resamples());

        let mut packets = drain(&mut conv, &data, &[13, 640, 977], 1);
        conv.finish(|p| packets.push(p.to_vec())).unwrap();

        let mut expected = Vec::new();
        quantize(&data, &mut expected);
        let flat: Vec<i16> = packets.concat();
        assert_eq!(flat, expected, "no sample altered, none dropped");
        assert_eq!(packets.len(), 10);
    }

    #[test]
    fn the_tail_shorter_than_a_packet_survives_the_flush() {
        // 16 100 mono frames = 25 full packets plus 100 samples. Without the
        // flush those 100 samples (6 ms of speech) would be lost.
        let data = sine(16_100, 1, 16_000, 440.0);
        let mut conv = Converter::new(16_000, 1).unwrap();
        let mut packets = drain(&mut conv, &data, &[512], 1);
        assert_eq!(packets.len(), 25);

        conv.finish(|p| packets.push(p.to_vec())).unwrap();
        assert_eq!(packets.len(), 26);

        let last = &packets[25];
        let mut expected_tail = Vec::new();
        quantize(&data[16_000..], &mut expected_tail);
        assert_eq!(&last[..100], &expected_tail[..]);
        assert!(
            last[100..].iter().all(|&s| s == 0),
            "zero-padded, not garbage"
        );
    }

    #[test]
    fn downmix_averages_channels_rather_than_taking_the_first() {
        // A stereo interface with the microphone on the right input: channel 0
        // is silence. Taking channel 0 would record nothing.
        let mut conv = Converter::new(16_000, 2).unwrap();
        let mut interleaved = Vec::new();
        for _ in 0..CHUNK_SAMPLES {
            interleaved.push(0.0);
            interleaved.push(1.0);
        }
        let mut packets = Vec::new();
        conv.push_interleaved(&interleaved, |p| packets.push(p.to_vec()))
            .unwrap();

        assert_eq!(packets.len(), 1);
        assert!(packets[0].iter().all(|&s| s == i16::MAX / 2));
    }

    #[test]
    fn resetting_clears_the_tail_so_recordings_do_not_bleed_into_each_other() {
        let mut conv = Converter::new(16_000, 1).unwrap();
        let mut dropped = Vec::new();
        conv.push_interleaved(&[0.5; 100], |p| dropped.push(p.to_vec()))
            .unwrap();
        assert!(dropped.is_empty(), "100 samples is not a packet yet");

        conv.reset();

        let mut packets = Vec::new();
        conv.push_interleaved(&[0.0; CHUNK_SAMPLES], |p| packets.push(p.to_vec()))
            .unwrap();
        assert_eq!(packets.len(), 1);
        assert!(
            packets[0].iter().all(|&s| s == 0),
            "the previous recording's 100 samples must not lead this packet"
        );
    }

    #[test]
    fn a_silent_capture_is_flagged_only_once_it_is_long_enough_to_judge() {
        let mut conv = Converter::new(48_000, 1).unwrap();
        assert!(!conv.input_was_silent(), "nothing observed yet");

        conv.push_interleaved(&[0.0; 1_000], |_| {}).unwrap();
        assert!(
            !conv.input_was_silent(),
            "21 ms of silence is a user who has not started speaking"
        );

        conv.push_interleaved(&[0.0; 24_000], |_| {}).unwrap();
        assert!(conv.input_was_silent(), "half a second of exact zeros");
    }

    #[test]
    fn one_non_zero_sample_anywhere_clears_the_silence_flag() {
        let mut conv = Converter::new(48_000, 1).unwrap();
        conv.push_interleaved(&[0.0; 24_000], |_| {}).unwrap();
        assert!(conv.input_was_silent());

        // A quiet room still has a noise floor; a blocked device does not.
        let mut late = vec![0.0f32; 24_000];
        late[23_999] = 1.0 / 32_768.0;
        conv.push_interleaved(&late, |_| {}).unwrap();
        assert!(
            !conv.input_was_silent(),
            "the guard must not scold a user who paused before speaking"
        );
    }

    #[test]
    fn resetting_clears_the_silence_verdict() {
        let mut conv = Converter::new(48_000, 1).unwrap();
        conv.push_interleaved(&[0.0; 24_000], |_| {}).unwrap();
        assert!(conv.input_was_silent());
        conv.reset();
        assert!(!conv.input_was_silent());
    }

    #[test]
    fn a_44100_hz_device_is_supported_even_though_the_ratio_is_not_integral() {
        // 44 100 : 16 000 reduces to 441:160 — not 3:1, and a common rate for
        // USB microphones. Construction must not fail and the output must be
        // close to the 1:2.75625 ratio.
        let data = sine(44_100, 1, 44_100, 440.0);
        let mut conv = Converter::new(44_100, 1).unwrap();
        let mut packets = drain(&mut conv, &data, &[512], 1);
        conv.finish(|p| packets.push(p.to_vec())).unwrap();

        // One second in, one second out: 25 packets, give or take the final
        // partial one the flush emits.
        assert!(
            (25..=26).contains(&packets.len()),
            "expected ~1 s of packets, got {}",
            packets.len()
        );
    }

    #[test]
    fn a_zero_sample_rate_is_rejected_rather_than_dividing_by_zero() {
        assert!(Converter::new(0, 1).is_err());
    }
}
