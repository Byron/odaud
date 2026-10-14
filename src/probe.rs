//! Deliberately separate source/capture processes for acoustic feasibility tests.

use std::{
    cell::UnsafeCell,
    f64::consts::TAU,
    ffi::c_void,
    fs::OpenOptions,
    io::{BufWriter, Write},
    mem::{MaybeUninit, align_of, offset_of, size_of},
    path::Path,
    ptr,
    sync::atomic::{AtomicU32, AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};

const INPUT: u32 = u32::from_be_bytes(*b"inpt");
const OUTPUT: u32 = u32::from_be_bytes(*b"outp");
const GLOBAL: u32 = u32::from_be_bytes(*b"glob");
const MAX_CHANNELS: usize = 64;
const MAX_CAPTURE_BYTES: usize = 64 * 1024 * 1024;

#[repr(C)]
struct Address {
    selector: u32,
    scope: u32,
    element: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Format {
    rate: f64,
    id: u32,
    flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels: u32,
    bits: u32,
    reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Buffer {
    channels: u32,
    bytes: u32,
    data: *mut c_void,
}

#[repr(C)]
struct BufferList {
    count: u32,
    buffers: [Buffer; 1],
}

#[repr(C)]
struct StreamUsage {
    proc: *mut c_void,
    count: u32,
    enabled: [u32; MAX_CHANNELS],
}

type IoProc = unsafe extern "C" fn(
    u32,
    *const c_void,
    *const BufferList,
    *const c_void,
    *mut BufferList,
    *const c_void,
    *mut c_void,
) -> i32;

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectGetPropertyDataSize(
        object: u32,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
    ) -> i32;
    fn AudioObjectGetPropertyData(
        object: u32,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
        data: *mut c_void,
    ) -> i32;
    fn AudioObjectSetPropertyData(
        object: u32,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: u32,
        data: *const c_void,
    ) -> i32;
    fn AudioDeviceCreateIOProcID(
        device: u32,
        callback: IoProc,
        state: *mut c_void,
        proc: *mut Option<IoProc>,
    ) -> i32;
    fn AudioDeviceStart(device: u32, proc: Option<IoProc>) -> i32;
    fn AudioDeviceStop(device: u32, proc: Option<IoProc>) -> i32;
    fn AudioDeviceDestroyIOProcID(device: u32, proc: Option<IoProc>) -> i32;
}

fn address(selector: &[u8; 4], scope: u32) -> Address {
    Address {
        selector: u32::from_be_bytes(*selector),
        scope,
        element: 0,
    }
}

fn status(code: i32, operation: &str) -> Result<(), String> {
    if code == 0 {
        Ok(())
    } else {
        Err(format!("{operation}: Core Audio OSStatus {code}"))
    }
}

fn property<T: Copy>(object: u32, selector: &[u8; 4], scope: u32) -> Result<T, String> {
    let mut value = MaybeUninit::<T>::uninit();
    let mut bytes = size_of::<T>() as u32;
    status(
        unsafe {
            AudioObjectGetPropertyData(
                object,
                &address(selector, scope),
                0,
                ptr::null(),
                &mut bytes,
                value.as_mut_ptr().cast(),
            )
        },
        "read audio property",
    )?;
    if bytes as usize != size_of::<T>() {
        return Err("Audio property changed size".into());
    }
    Ok(unsafe { value.assume_init() })
}

fn streams(device: u32, scope: u32) -> Result<Vec<u32>, String> {
    let address = address(b"stm#", scope);
    let mut bytes = 0;
    status(
        unsafe { AudioObjectGetPropertyDataSize(device, &address, 0, ptr::null(), &mut bytes) },
        "read stream count",
    )?;
    if bytes % 4 != 0 || bytes as usize > MAX_CHANNELS * 4 {
        return Err("Unsupported audio stream count".into());
    }
    let mut values = vec![0_u32; bytes as usize / 4];
    if bytes != 0 {
        let capacity = bytes;
        status(
            unsafe {
                AudioObjectGetPropertyData(
                    device,
                    &address,
                    0,
                    ptr::null(),
                    &mut bytes,
                    values.as_mut_ptr().cast(),
                )
            },
            "read streams",
        )?;
        if bytes != capacity {
            return Err("Audio streams changed during inspection".into());
        }
    }
    Ok(values)
}

fn format_channels(format: Format) -> Result<Vec<usize>, String> {
    let planar = format.flags & 32 != 0;
    let channels = format.channels as usize;
    let stride = if planar { 4 } else { channels * 4 };
    if format.id != u32::from_be_bytes(*b"lpcm")
        || format.flags & 1 == 0
        || format.flags & 2 != 0
        || format.flags & 4 != 0
        || format.flags & 8 == 0
        || format.bits != 32
        || channels == 0
        || channels > MAX_CHANNELS
        || format.bytes_per_frame as usize != stride
        || format.frames_per_packet != 1
        || format.bytes_per_packet as usize != stride
        || !format.rate.is_finite()
        || !(8_000.0..=768_000.0).contains(&format.rate)
    {
        return Err(
            "Probe requires native little-endian packed Float32 PCM, 1–64 channels, 8–768 kHz"
                .into(),
        );
    }
    Ok(if planar {
        vec![1; channels]
    } else {
        vec![channels]
    })
}

struct Layout {
    buffer_channels: Vec<usize>,
    channels: usize,
    rate: f64,
    max_frames: usize,
}

impl Layout {
    fn inspect(device: u32, scope: u32) -> Result<Self, String> {
        let stream_ids = streams(device, scope)?;
        if stream_ids.is_empty() {
            return Err("Selected device has no streams in this direction".into());
        }
        let mut rate = 0.0;
        let mut buffer_channels = Vec::new();
        for stream in stream_ids {
            let format: Format = property(stream, b"sfmt", GLOBAL)?;
            let channels = format_channels(format)?;
            if rate != 0.0 && rate != format.rate {
                return Err("Probe cannot use streams with different sample rates".into());
            }
            rate = format.rate;
            buffer_channels.extend(channels);
        }
        let channels = buffer_channels.iter().sum();
        if channels > MAX_CHANNELS {
            return Err("Probe supports at most 64 total channels".into());
        }
        let current: u32 = property(device, b"fsiz", GLOBAL)?;
        let variable = property::<u32>(device, b"vfsz", GLOBAL).unwrap_or(current);
        let max_frames = current.max(variable) as usize;
        if max_frames == 0 || max_frames > 65_536 {
            return Err("Unsupported maximum callback frame count".into());
        }

        #[repr(C)]
        struct Configuration {
            count: u32,
            buffers: [Buffer; MAX_CHANNELS],
        }
        let mut configuration = Configuration {
            count: 0,
            buffers: [Buffer {
                channels: 0,
                bytes: 0,
                data: ptr::null_mut(),
            }; MAX_CHANNELS],
        };
        let mut bytes = size_of::<Configuration>() as u32;
        status(
            unsafe {
                AudioObjectGetPropertyData(
                    device,
                    &address(b"slay", scope),
                    0,
                    ptr::null(),
                    &mut bytes,
                    (&mut configuration as *mut Configuration).cast(),
                )
            },
            "read stream buffer layout",
        )?;
        let count = configuration.count as usize;
        if count != buffer_channels.len()
            || count > MAX_CHANNELS
            || (bytes as usize) < offset_of!(Configuration, buffers) + count * size_of::<Buffer>()
            || configuration.buffers[..count]
                .iter()
                .map(|b| b.channels as usize)
                .ne(buffer_channels.iter().copied())
        {
            return Err("Stream formats do not match the device callback buffer layout".into());
        }
        Ok(Self {
            buffer_channels,
            channels,
            rate,
            max_frames,
        })
    }

    // HAL owns each buffer and guarantees its declared byte range for this callback.
    unsafe fn frames(&self, list: *const BufferList) -> Option<usize> {
        if list.is_null() || unsafe { (*list).count as usize } != self.buffer_channels.len() {
            return None;
        }
        let buffers = unsafe { ptr::addr_of!((*list).buffers).cast::<Buffer>() };
        let mut frames = None;
        for (index, &channels) in self.buffer_channels.iter().enumerate() {
            let buffer = unsafe { &*buffers.add(index) };
            let stride = channels * size_of::<f32>();
            if buffer.channels as usize != channels || buffer.bytes as usize % stride != 0 {
                return None;
            }
            let count = buffer.bytes as usize / stride;
            if count > self.max_frames
                || frames.is_some_and(|n| n != count)
                || !buffer.data.is_null() && (buffer.data as usize) % align_of::<f32>() != 0
            {
                return None;
            }
            frames = Some(count);
        }
        frames
    }
}

enum Audio {
    Source(Vec<f32>),
    Capture(Vec<f32>),
}

struct Realtime {
    audio: Audio,
    position: usize,
}

struct State {
    layout: Layout,
    target: usize,
    completed: AtomicUsize,
    failure: AtomicU32,
    realtime: UnsafeCell<Realtime>,
}

unsafe fn silence(list: *mut BufferList) {
    if list.is_null() {
        return;
    }
    let count = unsafe { (*list).count as usize };
    let buffers = unsafe { ptr::addr_of!((*list).buffers).cast::<Buffer>() };
    for index in 0..count.min(MAX_CHANNELS) {
        let buffer = unsafe { &*buffers.add(index) };
        if !buffer.data.is_null() && buffer.bytes as usize <= 65_536 * MAX_CHANNELS * 4 {
            unsafe {
                buffer
                    .data
                    .cast::<u8>()
                    .write_bytes(0, buffer.bytes as usize);
            }
        }
    }
}

unsafe extern "C" fn callback(
    _: u32,
    _: *const c_void,
    input: *const BufferList,
    _: *const c_void,
    output: *mut BufferList,
    _: *const c_void,
    client: *mut c_void,
) -> i32 {
    let state = unsafe { &*client.cast::<State>() };
    let realtime = unsafe { &mut *state.realtime.get() };
    let source = matches!(realtime.audio, Audio::Source(_));
    if state.failure.load(Ordering::Relaxed) != 0 {
        if source {
            unsafe {
                silence(output);
            }
        }
        return 0;
    }
    let list = if source { output.cast_const() } else { input };
    let Some(frames) = (unsafe { state.layout.frames(list) }) else {
        if source {
            unsafe {
                silence(output);
            }
        }
        state.failure.store(1, Ordering::Release);
        return 0;
    };
    let buffers = unsafe { ptr::addr_of!((*list).buffers).cast::<Buffer>() };
    let position = realtime.position;
    let copy_frames = frames.min(state.target.saturating_sub(position));
    let mut channel_offset = 0;
    for (index, &channels) in state.layout.buffer_channels.iter().enumerate() {
        let buffer = unsafe { &*buffers.add(index) };
        match &mut realtime.audio {
            Audio::Source(cycle) => {
                if buffer.data.is_null() {
                    state.failure.store(2, Ordering::Release);
                    continue;
                }
                let data = buffer.data.cast::<f32>();
                for frame in 0..frames {
                    let sample = if frame < copy_frames {
                        // The cycle is nonempty and source duration is bounded before registering IO.
                        unsafe { *cycle.as_ptr().add((position + frame) % cycle.len()) }
                    } else {
                        0.0
                    };
                    for channel in 0..channels {
                        unsafe {
                            data.add(frame * channels + channel).write(sample);
                        }
                    }
                }
            }
            Audio::Capture(samples) => {
                for frame in 0..copy_frames {
                    for channel in 0..channels {
                        let sample = if buffer.data.is_null() {
                            0.0
                        } else {
                            unsafe { *buffer.data.cast::<f32>().add(frame * channels + channel) }
                        };
                        let index =
                            (position + frame) * state.layout.channels + channel_offset + channel;
                        unsafe {
                            samples.as_mut_ptr().add(index).write(sample);
                        }
                    }
                }
            }
        }
        channel_offset += channels;
    }
    realtime.position += copy_frames;
    state.completed.store(realtime.position, Ordering::Release);
    0
}

fn disable_unused_streams(device: u32, proc: IoProc, scope: u32) -> Result<(), String> {
    let count = streams(device, scope)?.len();
    if count == 0 {
        return Ok(());
    }
    let usage = StreamUsage {
        proc: proc as *const () as *mut c_void,
        count: count as u32,
        enabled: [0; MAX_CHANNELS],
    };
    status(
        unsafe {
            AudioObjectSetPropertyData(
                device,
                &address(b"suse", scope),
                0,
                ptr::null(),
                (offset_of!(StreamUsage, enabled) + count * size_of::<u32>()) as u32,
                (&usage as *const StreamUsage).cast(),
            )
        },
        "disable unused IOProc stream direction",
    )
}

fn run(
    device: u32,
    state: Box<State>,
    seconds: f64,
    unused_scope: u32,
) -> Result<Box<State>, String> {
    let mut proc = None;
    let pointer = Box::into_raw(state);
    let creation =
        unsafe { AudioDeviceCreateIOProcID(device, callback, pointer.cast(), &mut proc) };
    if creation != 0 {
        unsafe {
            drop(Box::from_raw(pointer));
        }
        return Err(format!("Create IOProc: Core Audio OSStatus {creation}"));
    }
    let Some(proc_id) = proc else {
        // A successful registration without its removal token cannot safely release callback state.
        return Err(
            "Core Audio returned no IOProc token; callback state retained for safety".into(),
        );
    };
    let setup = disable_unused_streams(device, proc_id, unused_scope)
        .and_then(|()| status(unsafe { AudioDeviceStart(device, proc) }, "start audio IO"));
    if let Err(error) = setup {
        let stopped = unsafe { AudioDeviceStop(device, proc) };
        let destroyed = unsafe { AudioDeviceDestroyIOProcID(device, proc) };
        if stopped == 0 && destroyed == 0 {
            unsafe {
                drop(Box::from_raw(pointer));
            }
            return Err(error);
        }
        return Err(format!(
            "{error}; cleanup failed (stop {stopped}, destroy {destroyed}); callback state retained"
        ));
    }

    let state = unsafe { &*pointer };
    let deadline = Instant::now() + Duration::from_secs_f64(seconds + 10.0);
    let mut inspect_at = Instant::now() + Duration::from_millis(250);
    let mut error = None;
    while state.completed.load(Ordering::Acquire) < state.target {
        if crate::STOP.load(Ordering::Relaxed) {
            break;
        }
        if state.failure.load(Ordering::Acquire) != 0 {
            error = Some(
                "Audio callback layout changed or an output buffer was unavailable".to_string(),
            );
            break;
        }
        if Instant::now() >= deadline {
            error = Some(
                "Audio callback timed out; check device availability and microphone permission"
                    .to_string(),
            );
            break;
        }
        if Instant::now() >= inspect_at {
            let scope = if unused_scope == INPUT { OUTPUT } else { INPUT };
            match Layout::inspect(device, scope) {
                Ok(layout)
                    if layout.rate == state.layout.rate
                        && layout.buffer_channels == state.layout.buffer_channels
                        && layout.max_frames <= state.layout.max_frames => {}
                Ok(_) => {
                    error = Some("Audio device format or callback capacity changed".into());
                    break;
                }
                Err(reason) => {
                    error = Some(reason);
                    break;
                }
            }
            inspect_at = Instant::now() + Duration::from_millis(250);
        }
        thread::sleep(Duration::from_millis(10));
    }
    let stopped = unsafe { AudioDeviceStop(device, proc) };
    let destroyed = unsafe { AudioDeviceDestroyIOProcID(device, proc) };
    if stopped != 0 || destroyed != 0 {
        return Err(format!(
            "Audio teardown failed (stop {stopped}, destroy {destroyed}); callback state retained for safety"
        ));
    }
    let state = unsafe { Box::from_raw(pointer) };
    if state.failure.load(Ordering::Acquire) != 0 {
        return Err("Audio callback layout changed or an output buffer was unavailable".into());
    }
    if let Some(error) = error {
        return Err(error);
    }
    Ok(state)
}

fn frame_count(seconds: f64, rate: f64, maximum: f64) -> Result<usize, String> {
    if !seconds.is_finite() || seconds <= 0.0 || seconds > maximum {
        return Err(format!(
            "Duration must be finite, positive and at most {maximum} seconds"
        ));
    }
    let frames = (seconds * rate).round();
    if !frames.is_finite() || frames < 1.0 || frames > usize::MAX as f64 {
        return Err("Duration cannot be represented in audio frames".into());
    }
    Ok(frames as usize)
}

fn chirp_cycle(rate: f64) -> Vec<f32> {
    let mut cycle = vec![0.0; rate.round() as usize];
    let start = (rate * 0.25).round() as usize;
    let length = (rate * 0.020).round() as usize;
    for frame in 0..length {
        let time = frame as f64 / rate;
        let envelope = 0.5 - 0.5 * (TAU * frame as f64 / (length - 1) as f64).cos();
        let phase = TAU * (700.0 * time + 0.5 * (2800.0 - 700.0) / 0.020 * time * time);
        cycle[start + frame] = (0.05 * envelope * phase.sin()) as f32;
    }
    cycle
}

/// Play a quiet 20 ms chirp each second, mirrored to every output channel.
pub fn play(device: u32, seconds: f64) -> Result<String, String> {
    let layout = Layout::inspect(device, OUTPUT)?;
    let target = frame_count(seconds, layout.rate, 86_400.0)?;
    let audio = Audio::Source(chirp_cycle(layout.rate));
    let state = Box::new(State {
        layout,
        target,
        completed: AtomicUsize::new(0),
        failure: AtomicU32::new(0),
        realtime: UnsafeCell::new(Realtime { audio, position: 0 }),
    });
    let state = run(device, state, seconds, INPUT)?;
    Ok(format!(
        "Played {} frames at {} Hz to {} channels; 20 ms, 700–2800 Hz chirp every second, peak at most 0.05 (-26 dBFS)",
        state.completed.load(Ordering::Relaxed),
        state.layout.rate,
        state.layout.channels
    ))
}

/// Capture at most 60 seconds for acoustic measurement; never overwrite a file.
pub fn capture(device: u32, seconds: f64, path: &Path) -> Result<String, String> {
    let layout = Layout::inspect(device, INPUT)?;
    let target = frame_count(seconds, layout.rate, 60.0)?;
    let samples = target
        .checked_mul(layout.channels)
        .filter(|n| *n <= MAX_CAPTURE_BYTES / size_of::<f32>())
        .ok_or("Capture would exceed the 64 MiB memory limit")?;
    if layout.rate.fract() != 0.0 {
        return Err("PCM WAV requires an integer sample rate".into());
    }
    if path.exists() {
        return Err(format!("Refusing to overwrite {}", path.display()));
    }
    let state = Box::new(State {
        layout,
        target,
        completed: AtomicUsize::new(0),
        failure: AtomicU32::new(0),
        realtime: UnsafeCell::new(Realtime {
            audio: Audio::Capture(vec![0.0; samples]),
            position: 0,
        }),
    });
    let state = run(device, state, seconds, OUTPUT)?;
    let rate = state.layout.rate as u32;
    let channels = state.layout.channels as u16;
    let captured = state.completed.load(Ordering::Relaxed);
    let Audio::Capture(mut samples) = state.realtime.into_inner().audio else {
        unreachable!()
    };
    samples.truncate(captured * channels as usize);
    let peak = samples
        .iter()
        .filter(|s| s.is_finite())
        .fold(0.0_f32, |p, s| p.max(s.abs()));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("Create {}: {e}", path.display()))?;
    let mut output = BufWriter::new(file);
    write_wav(&mut output, rate, channels, &samples)
        .and_then(|()| output.flush())
        .map_err(|e| format!("Write {}: {e}", path.display()))?;
    Ok(format!(
        "Captured {captured} frames, {rate} Hz, {channels} channels to {}; Float32 input peak {peak:.6}{}",
        path.display(),
        if peak == 0.0 {
            "; all-zero audio is not proof of working microphone access"
        } else {
            ""
        }
    ))
}

fn write_wav(
    output: &mut impl Write,
    rate: u32,
    channels: u16,
    samples: &[f32],
) -> std::io::Result<()> {
    let bytes = (samples.len() * 2) as u32;
    output.write_all(b"RIFF")?;
    output.write_all(&(36 + bytes).to_le_bytes())?;
    output.write_all(b"WAVEfmt ")?;
    output.write_all(&16_u32.to_le_bytes())?;
    output.write_all(&1_u16.to_le_bytes())?;
    output.write_all(&channels.to_le_bytes())?;
    output.write_all(&rate.to_le_bytes())?;
    output.write_all(&(rate * channels as u32 * 2).to_le_bytes())?;
    output.write_all(&(channels * 2).to_le_bytes())?;
    output.write_all(&16_u16.to_le_bytes())?;
    output.write_all(b"data")?;
    output.write_all(&bytes.to_le_bytes())?;
    for &sample in samples {
        let sample = if sample.is_finite() {
            sample.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        output.write_all(&((sample * 32767.0).round() as i16).to_le_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(channels: Vec<usize>) -> Layout {
        Layout {
            channels: channels.iter().sum(),
            buffer_channels: channels,
            rate: 48_000.0,
            max_frames: 16,
        }
    }

    #[repr(C)]
    struct Pair {
        count: u32,
        buffers: [Buffer; 2],
    }

    unsafe fn invoke(state: &State, input: *const BufferList, output: *mut BufferList) {
        unsafe {
            callback(
                0,
                ptr::null(),
                input,
                ptr::null(),
                output,
                ptr::null(),
                (state as *const State).cast_mut().cast(),
            );
        }
    }

    #[test]
    fn capture_planar_channels_and_partial_final_buffer() {
        let mut left = [1.0_f32, 2.0, 3.0];
        let mut right = [4.0_f32, 5.0, 6.0];
        let input = Pair {
            count: 2,
            buffers: [
                Buffer {
                    channels: 1,
                    bytes: 12,
                    data: left.as_mut_ptr().cast(),
                },
                Buffer {
                    channels: 1,
                    bytes: 12,
                    data: right.as_mut_ptr().cast(),
                },
            ],
        };
        let state = State {
            layout: layout(vec![1, 1]),
            target: 2,
            completed: AtomicUsize::new(0),
            failure: AtomicU32::new(0),
            realtime: UnsafeCell::new(Realtime {
                audio: Audio::Capture(vec![0.0; 4]),
                position: 0,
            }),
        };
        unsafe {
            invoke(&state, (&input as *const Pair).cast(), ptr::null_mut());
        }
        assert_eq!(state.completed.load(Ordering::Relaxed), 2);
        let Audio::Capture(samples) = state.realtime.into_inner().audio else {
            unreachable!()
        };
        assert_eq!(samples, [1.0, 4.0, 2.0, 5.0]);
    }

    #[test]
    fn source_wraps_cycle_and_silences_after_requested_duration() {
        let mut samples = [99.0_f32; 8];
        let mut output = BufferList {
            count: 1,
            buffers: [Buffer {
                channels: 2,
                bytes: 32,
                data: samples.as_mut_ptr().cast(),
            }],
        };
        let state = State {
            layout: layout(vec![2]),
            target: 3,
            completed: AtomicUsize::new(0),
            failure: AtomicU32::new(0),
            realtime: UnsafeCell::new(Realtime {
                audio: Audio::Source(vec![0.1, -0.2]),
                position: 0,
            }),
        };
        unsafe {
            invoke(&state, ptr::null(), &mut output);
        }
        assert_eq!(samples, [0.1, 0.1, -0.2, -0.2, 0.1, 0.1, 0.0, 0.0]);
        unsafe {
            invoke(&state, ptr::null(), &mut output);
        }
        assert_eq!(samples, [0.0; 8]);
    }

    #[test]
    fn layout_change_silences_source_and_latches_failure() {
        let mut samples = [0.5_f32; 4];
        let mut output = BufferList {
            count: 1,
            buffers: [Buffer {
                channels: 1,
                bytes: 16,
                data: samples.as_mut_ptr().cast(),
            }],
        };
        let state = State {
            layout: layout(vec![2]),
            target: 3,
            completed: AtomicUsize::new(0),
            failure: AtomicU32::new(0),
            realtime: UnsafeCell::new(Realtime {
                audio: Audio::Source(vec![0.1]),
                position: 0,
            }),
        };
        unsafe {
            invoke(&state, ptr::null(), &mut output);
        }
        assert_eq!(samples, [0.0; 4]);
        assert_ne!(state.failure.load(Ordering::Relaxed), 0);
        output.buffers[0].channels = 2;
        unsafe {
            invoke(&state, ptr::null(), &mut output);
        }
        assert_eq!(samples, [0.0; 4]);
        assert_eq!(state.completed.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn rejects_invalid_sizes_formats_and_duration() {
        let input = BufferList {
            count: 1,
            buffers: [Buffer {
                channels: 2,
                bytes: 7,
                data: ptr::null_mut(),
            }],
        };
        assert_eq!(unsafe { layout(vec![2]).frames(&input) }, None);
        assert!(format_channels(Format::default()).is_err());
        for seconds in [f64::NAN, f64::INFINITY, -1.0, 0.0, 61.0] {
            assert!(frame_count(seconds, 48_000.0, 60.0).is_err());
        }
        assert_eq!(frame_count(0.1, 48_000.0, 60.0).unwrap(), 4_800);
        let chirp = chirp_cycle(48_000.0);
        assert_eq!(chirp.len(), 48_000);
        assert!(chirp.iter().all(|s| s.is_finite() && s.abs() <= 0.05));
        assert!(chirp[..12_000].iter().all(|s| *s == 0.0));
        assert!(chirp.iter().any(|s| s.abs() > 0.01));
    }

    #[test]
    fn wav_header_and_pcm_are_consistent() {
        let mut wav = Vec::new();
        write_wav(&mut wav, 48_000, 2, &[0.0, 1.0, -1.0, f32::NAN]).unwrap();
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()), 44);
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 8);
        assert_eq!(&wav[44..], &[0, 0, 255, 127, 1, 128, 0, 0]);
    }
}
