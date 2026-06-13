// WASAPI audio capture for screen recording.
// Supports system loopback and microphone capture, with mixing when both are enabled.

use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};

use windows::Win32::{
    Media::Audio::{
        eCapture, eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator,
        MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED,
        AUDCLNT_STREAMFLAGS_LOOPBACK, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
    },
    System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED},
};
use windows_capture::encoder::VideoEncoder;

// Raw WAVE format tag constants (avoids pulling in extra features).
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

// SubFormat GUIDs for WAVEFORMATEXTENSIBLE.
const SUBTYPE_IEEE_FLOAT: windows::core::GUID =
    windows::core::GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);

pub type SharedEncoder = Arc<Mutex<Option<VideoEncoder>>>;

// Target encoder format: 48 kHz, stereo, 16-bit signed PCM.
pub const TARGET_SAMPLE_RATE: u32 = 48_000;
pub const TARGET_CHANNELS: u32 = 2;

pub struct WasapiAudio {
    stop: Arc<AtomicBool>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl WasapiAudio {
    /// Start WASAPI audio capture. Spawns background threads that push PCM
    /// samples into `encoder`. Does nothing if neither system nor mic is enabled.
    pub fn start(encoder: SharedEncoder, capture_system: bool, capture_mic: bool) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();

        if !capture_system && !capture_mic {
            return Self { stop, threads };
        }

        if capture_system && capture_mic {
            // Both sources: capture → mpsc channels → mixer thread → encoder.
            let (sys_tx, sys_rx) = mpsc::channel::<Vec<i16>>();
            let (mic_tx, mic_rx) = mpsc::channel::<Vec<i16>>();

            let stop_sys = stop.clone();
            let stop_mic = stop.clone();
            let enc = encoder.clone();

            threads.push(std::thread::spawn(move || {
                capture_wasapi(true, stop_sys, sys_tx);
            }));
            threads.push(std::thread::spawn(move || {
                capture_wasapi(false, stop_mic, mic_tx);
            }));
            threads.push(std::thread::spawn(move || {
                mix_and_send(enc, sys_rx, mic_rx);
            }));
        } else {
            // Single source: capture → mpsc channel → relay thread → encoder.
            let (tx, rx) = mpsc::channel::<Vec<i16>>();
            let stop_c = stop.clone();
            let is_system = capture_system;
            let enc = encoder.clone();

            threads.push(std::thread::spawn(move || {
                capture_wasapi(is_system, stop_c, tx);
            }));
            threads.push(std::thread::spawn(move || {
                relay_to_encoder(enc, rx);
            }));
        }

        Self { stop, threads }
    }

    /// Signal all capture threads to stop and wait for them to exit.
    pub fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        for t in self.threads {
            t.join().ok();
        }
    }
}

// ─── Capture thread ───────────────────────────────────────────────────────────

fn capture_wasapi(is_loopback: bool, stop: Arc<AtomicBool>, tx: mpsc::Sender<Vec<i16>>) {
    unsafe {
        // COM must be initialised per thread.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let Ok(enumerator) = CoCreateInstance::<_, IMMDeviceEnumerator>(
            &MMDeviceEnumerator,
            None,
            CLSCTX_ALL,
        ) else { return; };

        let data_flow = if is_loopback { eRender } else { eCapture };
        let Ok(device) = enumerator.GetDefaultAudioEndpoint(data_flow, eConsole) else { return; };

        let Ok(audio_client) = device.Activate::<IAudioClient>(CLSCTX_ALL, None) else { return; };

        // Query mix format — this is the format WASAPI uses internally.
        let Ok(mix_fmt_ptr) = audio_client.GetMixFormat() else { return; };
        let mix_fmt: &WAVEFORMATEX = &*mix_fmt_ptr;
        let channels = mix_fmt.nChannels as usize;
        let bits = mix_fmt.wBitsPerSample as usize;
        let is_float = is_ieee_float(mix_fmt);

        let stream_flags = if is_loopback { AUDCLNT_STREAMFLAGS_LOOPBACK } else { 0 };
        // 1-second reference-time buffer in 100 ns units.
        if audio_client
            .Initialize(AUDCLNT_SHAREMODE_SHARED, stream_flags, 10_000_000, 0, mix_fmt_ptr, None)
            .is_err()
        {
            CoTaskMemFree(Some(mix_fmt_ptr as *const _ as *const _));
            return;
        }

        CoTaskMemFree(Some(mix_fmt_ptr as *const _ as *const _));

        let Ok(capture_client) = audio_client.GetService::<IAudioCaptureClient>() else { return; };

        audio_client.Start().ok();

        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(10));

            loop {
                let Ok(packet_size) = capture_client.GetNextPacketSize() else { break; };
                if packet_size == 0 {
                    break;
                }

                let mut data_ptr: *mut u8 = std::ptr::null_mut();
                let mut num_frames: u32 = 0;
                let mut flags: u32 = 0;

                if capture_client
                    .GetBuffer(&mut data_ptr, &mut num_frames, &mut flags, None, None)
                    .is_err()
                {
                    break;
                }

                let num_samples = num_frames as usize * channels;
                let samples = if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 {
                    vec![0i16; num_samples]
                } else if is_float && bits == 32 {
                    let src = std::slice::from_raw_parts(data_ptr as *const f32, num_samples);
                    src.iter()
                        .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
                        .collect()
                } else if !is_float && bits == 16 {
                    let src = std::slice::from_raw_parts(data_ptr as *const i16, num_samples);
                    src.to_vec()
                } else if !is_float && bits == 32 {
                    // 32-bit int PCM: scale down to 16-bit.
                    let src = std::slice::from_raw_parts(data_ptr as *const i32, num_samples);
                    src.iter().map(|&s| (s >> 16) as i16).collect()
                } else {
                    vec![0i16; num_samples]
                };

                capture_client.ReleaseBuffer(num_frames).ok();

                // Remix channels to match TARGET_CHANNELS (stereo).
                let samples = remix_channels(samples, channels, TARGET_CHANNELS as usize);

                if tx.send(samples).is_err() {
                    break;
                }
            }
        }

        audio_client.Stop().ok();
    }
}

// Determine whether a WAVEFORMATEX (or WAVEFORMATEXTENSIBLE) uses IEEE float.
fn is_ieee_float(fmt: &WAVEFORMATEX) -> bool {
    if fmt.wFormatTag == WAVE_FORMAT_IEEE_FLOAT {
        return true;
    }
    if fmt.wFormatTag == WAVE_FORMAT_EXTENSIBLE {
        // SAFETY: WASAPI guarantees the buffer is large enough for WAVEFORMATEXTENSIBLE
        // when wFormatTag == WAVE_FORMAT_EXTENSIBLE.
        // Use read_unaligned because WAVEFORMATEXTENSIBLE is packed(1).
        let sub_format = unsafe {
            let ext = fmt as *const WAVEFORMATEX as *const WAVEFORMATEXTENSIBLE;
            std::ptr::read_unaligned(std::ptr::addr_of!((*ext).SubFormat))
        };
        return sub_format == SUBTYPE_IEEE_FLOAT;
    }
    false
}

// Convert channel count. Handles mono→stereo and multi→stereo downmix.
fn remix_channels(samples: Vec<i16>, src_ch: usize, dst_ch: usize) -> Vec<i16> {
    if src_ch == dst_ch {
        return samples;
    }
    let frames = samples.len() / src_ch;

    if dst_ch == 2 && src_ch == 1 {
        // Mono → stereo: duplicate each sample.
        let mut out = Vec::with_capacity(frames * 2);
        for s in &samples {
            out.push(*s);
            out.push(*s);
        }
        return out;
    }

    if dst_ch == 2 && src_ch >= 2 {
        // Take first two channels (L/R from a surround mix).
        let mut out = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            out.push(samples[i * src_ch]);
            out.push(samples[i * src_ch + 1]);
        }
        return out;
    }

    // Fallback: truncate or zero-pad to dst_ch per frame.
    let mut out = Vec::with_capacity(frames * dst_ch);
    for i in 0..frames {
        for ch in 0..dst_ch {
            out.push(if ch < src_ch { samples[i * src_ch + ch] } else { 0 });
        }
    }
    out
}

// ─── Single-source relay ───────────────────────────────────────────────────────

fn relay_to_encoder(encoder: SharedEncoder, rx: mpsc::Receiver<Vec<i16>>) {
    while let Ok(samples) = rx.recv() {
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        if let Some(enc) = encoder.lock().unwrap().as_mut() {
            enc.send_audio_buffer(&bytes, 0).ok();
        }
    }
}

// ─── Two-source mixer ─────────────────────────────────────────────────────────

fn mix_and_send(
    encoder: SharedEncoder,
    sys_rx: mpsc::Receiver<Vec<i16>>,
    mic_rx: mpsc::Receiver<Vec<i16>>,
) {
    let mut sys_buf: VecDeque<i16> = VecDeque::new();
    let mut mic_buf: VecDeque<i16> = VecDeque::new();

    loop {
        // Block on system audio (primary source); poll mic.
        match sys_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(samples) => sys_buf.extend(samples),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        while let Ok(samples) = mic_rx.try_recv() {
            mic_buf.extend(samples);
        }

        // Mix as many frames as both buffers have in common.
        let avail = sys_buf.len().min(mic_buf.len());
        if avail == 0 {
            continue;
        }

        let mixed: Vec<i16> = (0..avail)
            .map(|_| {
                let s = sys_buf.pop_front().unwrap_or(0) as i32;
                let m = mic_buf.pop_front().unwrap_or(0) as i32;
                ((s + m) / 2).clamp(-32768, 32767) as i16
            })
            .collect();

        let bytes: Vec<u8> = mixed.iter().flat_map(|s| s.to_le_bytes()).collect();
        if let Some(enc) = encoder.lock().unwrap().as_mut() {
            enc.send_audio_buffer(&bytes, 0).ok();
        }
    }
}
