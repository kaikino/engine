//! System audio output.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Keep at most this many frames of audio queued, to bound latency.
const MAX_QUEUED_FRAMES: usize = 6;

/// A queue of 44.1 kHz mono samples feeding the audio device.
#[derive(Clone, Default)]
pub struct SampleQueue {
    inner: Arc<Mutex<VecDeque<i16>>>,
}

impl SampleQueue {
    /// Queues a frame of samples, dropping the oldest if we are far ahead.
    pub fn push(&self, samples: &[i16]) {
        let mut q = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        q.extend(samples.iter().copied());
        let max = MAX_QUEUED_FRAMES * kagami::apu::SAMPLES_PER_FRAME;
        while q.len() > max {
            q.pop_front();
        }
    }

    /// Takes one sample, or silence if the queue ran dry.
    pub fn pop(&self) -> i16 {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front()
            .unwrap_or(0)
    }

    /// Samples waiting.
    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// `true` if nothing is queued.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Resamples the 44.1 kHz mono queue to the device's rate and channel count
/// using nearest-neighbour stepping. Shared by the real backend and tests.
pub struct Resampler {
    queue: SampleQueue,
    device_rate: u32,
    channels: usize,
    acc: u32,
    last: i16,
}

impl Resampler {
    /// Creates a resampler for a device.
    pub fn new(queue: SampleQueue, device_rate: u32, channels: usize) -> Resampler {
        Resampler {
            queue,
            device_rate: device_rate.max(1),
            channels: channels.max(1),
            acc: 0,
            last: 0,
        }
    }

    /// Fills an interleaved f32 buffer.
    pub fn fill(&mut self, out: &mut [f32]) {
        for frame in out.chunks_mut(self.channels) {
            self.acc += kagami::apu::SAMPLE_RATE;
            while self.acc >= self.device_rate {
                self.acc -= self.device_rate;
                self.last = self.queue.pop();
            }
            let v = f32::from(self.last) / 32768.0;
            for s in frame {
                *s = v;
            }
        }
    }
}

/// A live audio stream. Dropping it stops playback.
pub struct AudioOutput {
    #[cfg(feature = "audio")]
    _stream: cpal::Stream,
    queue: SampleQueue,
}

impl AudioOutput {
    /// Opens the default output device.
    ///
    /// # Errors
    /// Returns a message if there is no device or the stream cannot start,
    /// or if the crate was built without the `audio` feature.
    #[cfg(feature = "audio")]
    pub fn start() -> Result<AudioOutput, String> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no audio output device")?;
        let config = device.default_output_config().map_err(|e| e.to_string())?;
        let rate = config.sample_rate();
        let channels = usize::from(config.channels());
        let queue = SampleQueue::default();
        let mut resampler = Resampler::new(queue.clone(), rate, channels);
        let stream = device
            .build_output_stream(
                config.into(),
                move |data: &mut [f32], _| resampler.fill(data),
                |err| eprintln!("kagami audio: {err}"),
                None,
            )
            .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(AudioOutput {
            _stream: stream,
            queue,
        })
    }

    /// Always fails: built without the `audio` feature.
    #[cfg(not(feature = "audio"))]
    pub fn start() -> Result<AudioOutput, String> {
        Err("built without the audio feature".to_string())
    }

    /// The queue to feed from the simulation thread.
    pub fn queue(&self) -> SampleQueue {
        self.queue.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_bounds_latency() {
        let q = SampleQueue::default();
        assert!(q.is_empty());
        let frame = vec![1000i16; kagami::apu::SAMPLES_PER_FRAME];
        for _ in 0..10 {
            q.push(&frame);
        }
        assert_eq!(q.len(), MAX_QUEUED_FRAMES * kagami::apu::SAMPLES_PER_FRAME);
        assert_eq!(q.pop(), 1000);
        let empty = SampleQueue::default();
        assert_eq!(empty.pop(), 0);
    }

    #[test]
    fn resampler_steps_through_queue() {
        let q = SampleQueue::default();
        let ramp: Vec<i16> = (0..100).map(|i| i * 100).collect();
        q.push(&ramp);
        // Same rate, stereo: each source sample appears once per stereo frame.
        let mut r = Resampler::new(q.clone(), 44_100, 2);
        let mut out = vec![0f32; 8];
        r.fill(&mut out);
        assert!((out[0] - 0.0).abs() < 1e-6);
        assert!((out[2] - 100.0 / 32768.0).abs() < 1e-6);
        assert!((out[2] - out[3]).abs() < 1e-9);
        assert_eq!(q.len(), 96);
        // Double rate, mono: every source sample is held for two outputs.
        let q2 = SampleQueue::default();
        q2.push(&ramp);
        let mut r2 = Resampler::new(q2.clone(), 88_200, 1);
        let mut out = vec![0f32; 6];
        r2.fill(&mut out);
        // Outputs come in held pairs after a one-sample startup:
        // [0, s0, s0, s1, s1, s2], so pairs are equal and neighbours differ.
        assert!((out[1] - out[2]).abs() < 1e-9);
        assert!((out[3] - out[4]).abs() < 1e-9);
        assert!((out[2] - out[3]).abs() > 1e-6);
        // Dry queue plays silence.
        let mut r3 = Resampler::new(SampleQueue::default(), 48_000, 1);
        let mut out = vec![1f32; 4];
        r3.fill(&mut out);
        assert!(out.iter().all(|&s| s.abs() < 1e-9));
        let _ = Resampler::new(SampleQueue::default(), 0, 0);
    }
}
