//! Core Audio control-thread operations. No functions here belong in an audio callback.

use std::{
    ffi::c_void,
    fmt::Write,
    mem, ptr, thread,
    time::{Duration, Instant},
};

type CfRef = *const c_void;
type Status = i32;

pub const fn fourcc(value: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*value)
}
const GLOBAL: u32 = fourcc(b"glob");
const OUTPUT: u32 = fourcc(b"outp");
const INPUT: u32 = fourcc(b"inpt");
const SYSTEM: u32 = 1;
const UTF8: u32 = 0x08000100;
const MAX_PROPERTY_BYTES: usize = 1024 * 1024;

#[repr(C)]
struct Address {
    selector: u32,
    scope: u32,
    element: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct AudioBuffer {
    channels: u32,
    byte_size: u32,
    data: *mut c_void,
}

#[repr(C)]
struct AudioBufferList {
    count: u32,
    buffers: [AudioBuffer; 1],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct StreamFormat {
    pub sample_rate: f64,
    pub format_id: u32,
    pub format_flags: u32,
    pub bytes_per_packet: u32,
    pub frames_per_packet: u32,
    pub bytes_per_frame: u32,
    pub channels_per_frame: u32,
    pub bits_per_channel: u32,
    pub reserved: u32,
}

type Retain = Option<unsafe extern "C" fn(CfRef, CfRef) -> CfRef>;
type Release = Option<unsafe extern "C" fn(CfRef, CfRef)>;
type Description = Option<unsafe extern "C" fn(CfRef) -> CfRef>;
type Equal = Option<unsafe extern "C" fn(CfRef, CfRef) -> u8>;

#[repr(C)]
struct ArrayCallbacks {
    version: isize,
    retain: Retain,
    release: Release,
    description: Description,
    equal: Equal,
}
#[repr(C)]
struct DictionaryKeyCallbacks {
    version: isize,
    retain: Retain,
    release: Release,
    description: Description,
    equal: Equal,
    hash: Option<unsafe extern "C" fn(CfRef) -> usize>,
}
#[repr(C)]
struct DictionaryValueCallbacks {
    version: isize,
    retain: Retain,
    release: Release,
    description: Description,
    equal: Equal,
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFTypeArrayCallBacks: ArrayCallbacks;
    static kCFTypeDictionaryKeyCallBacks: DictionaryKeyCallbacks;
    static kCFTypeDictionaryValueCallBacks: DictionaryValueCallbacks;
    fn CFRelease(value: CfRef);
    fn CFGetTypeID(value: CfRef) -> usize;
    fn CFCopyDescription(value: CfRef) -> CfRef;
    fn CFStringGetTypeID() -> usize;
    fn CFStringCreateWithBytes(
        allocator: CfRef,
        bytes: *const u8,
        count: isize,
        encoding: u32,
        external: u8,
    ) -> CfRef;
    fn CFStringGetLength(value: CfRef) -> isize;
    fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
    fn CFStringGetCString(value: CfRef, buffer: *mut u8, capacity: isize, encoding: u32) -> u8;
    fn CFNumberCreate(allocator: CfRef, kind: isize, value: *const c_void) -> CfRef;
    fn CFNumberGetTypeID() -> usize;
    fn CFNumberGetValue(number: CfRef, kind: isize, value: *mut c_void) -> u8;
    fn CFBooleanGetTypeID() -> usize;
    fn CFBooleanGetValue(boolean: CfRef) -> u8;
    fn CFArrayGetTypeID() -> usize;
    fn CFArrayCreate(
        allocator: CfRef,
        values: *const CfRef,
        count: isize,
        callbacks: *const ArrayCallbacks,
    ) -> CfRef;
    fn CFArrayGetCount(array: CfRef) -> isize;
    fn CFArrayGetValueAtIndex(array: CfRef, index: isize) -> CfRef;
    fn CFDictionaryGetTypeID() -> usize;
    fn CFDictionaryCreateMutable(
        allocator: CfRef,
        capacity: isize,
        keys: *const DictionaryKeyCallbacks,
        values: *const DictionaryValueCallbacks,
    ) -> CfRef;
    fn CFDictionaryCreateMutableCopy(allocator: CfRef, capacity: isize, source: CfRef) -> CfRef;
    fn CFDictionarySetValue(dictionary: CfRef, key: CfRef, value: CfRef);
    fn CFDictionaryGetValue(dictionary: CfRef, key: CfRef) -> CfRef;
}

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectGetPropertyDataSize(
        id: u32,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
    ) -> Status;
    fn AudioObjectGetPropertyData(
        id: u32,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
        data: *mut c_void,
    ) -> Status;
    fn AudioObjectSetPropertyData(
        id: u32,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: u32,
        data: *const c_void,
    ) -> Status;
    fn AudioObjectIsPropertySettable(id: u32, address: *const Address, settable: *mut u8)
    -> Status;
    fn AudioHardwareCreateAggregateDevice(description: CfRef, id: *mut u32) -> Status;
    fn AudioHardwareDestroyAggregateDevice(id: u32) -> Status;
}

unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
    fn __error() -> *mut i32;
}

struct Cf(CfRef);
impl Cf {
    fn owned(value: CfRef) -> Result<Self, String> {
        if value.is_null() {
            Err("Core Foundation returned a null object".into())
        } else {
            Ok(Self(value))
        }
    }
    fn string(value: &str) -> Result<Self, String> {
        Self::owned(unsafe {
            CFStringCreateWithBytes(ptr::null(), value.as_ptr(), value.len() as isize, UTF8, 0)
        })
    }
    fn integer(value: i32) -> Result<Self, String> {
        Self::owned(unsafe { CFNumberCreate(ptr::null(), 3, ptr::from_ref(&value).cast()) })
    }
    fn number(value: f64) -> Result<Self, String> {
        Self::owned(unsafe { CFNumberCreate(ptr::null(), 6, ptr::from_ref(&value).cast()) })
    }
    fn dictionary() -> Result<Self, String> {
        Self::owned(unsafe {
            CFDictionaryCreateMutable(
                ptr::null(),
                0,
                &kCFTypeDictionaryKeyCallBacks,
                &kCFTypeDictionaryValueCallBacks,
            )
        })
    }
    fn array(values: &[Cf]) -> Result<Self, String> {
        let pointers: Vec<_> = values.iter().map(|value| value.0).collect();
        Self::owned(unsafe {
            CFArrayCreate(
                ptr::null(),
                pointers.as_ptr(),
                pointers.len() as isize,
                &kCFTypeArrayCallBacks,
            )
        })
    }
    fn set(&mut self, key: &str, value: &Cf) -> Result<(), String> {
        let key = Self::string(key)?;
        unsafe { CFDictionarySetValue(self.0, key.0, value.0) };
        Ok(())
    }
    fn description(&self) -> Result<String, String> {
        let description = Self::owned(unsafe { CFCopyDescription(self.0) })?;
        cf_string(description.0)
    }
}
impl Drop for Cf {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) };
    }
}

fn cf_string(value: CfRef) -> Result<String, String> {
    if value.is_null() || unsafe { CFGetTypeID(value) != CFStringGetTypeID() } {
        return Err("Core Audio returned a non-string value".into());
    }
    let capacity = unsafe { CFStringGetMaximumSizeForEncoding(CFStringGetLength(value), UTF8) };
    if capacity < 0 || capacity as usize >= MAX_PROPERTY_BYTES {
        return Err("Core Audio string is too large".into());
    }
    let mut buffer = vec![0u8; capacity as usize + 1];
    if unsafe { CFStringGetCString(value, buffer.as_mut_ptr(), buffer.len() as isize, UTF8) } == 0 {
        return Err("Core Audio string cannot be converted to UTF-8".into());
    }
    let length = buffer
        .iter()
        .position(|&byte| byte == 0)
        .ok_or("Core Audio string lacks terminator")?;
    String::from_utf8(buffer[..length].to_vec()).map_err(|error| error.to_string())
}

fn dictionary_value(dictionary: CfRef, key: &str) -> Result<CfRef, String> {
    if dictionary.is_null() || unsafe { CFGetTypeID(dictionary) != CFDictionaryGetTypeID() } {
        return Err("aggregate composition contains a non-dictionary".into());
    }
    let key_object = Cf::string(key)?;
    let value = unsafe { CFDictionaryGetValue(dictionary, key_object.0) };
    if value.is_null() {
        return Err(format!("aggregate composition is missing {key:?}"));
    }
    Ok(value)
}

fn cf_number(value: CfRef) -> Result<f64, String> {
    if value.is_null() {
        return Err("aggregate composition contains a null number".into());
    }
    if unsafe { CFGetTypeID(value) == CFBooleanGetTypeID() } {
        return Ok(f64::from(unsafe { CFBooleanGetValue(value) }));
    }
    let mut number = 0.0f64;
    if unsafe { CFGetTypeID(value) != CFNumberGetTypeID() }
        || unsafe { CFNumberGetValue(value, 6, ptr::from_mut(&mut number).cast()) } == 0
        || !number.is_finite()
    {
        return Err("aggregate composition contains an invalid number".into());
    }
    Ok(number)
}

fn compare_fields(
    actual: CfRef,
    expected: CfRef,
    strings: &[&str],
    numbers: &[&str],
) -> Result<(), String> {
    for key in strings {
        let actual = cf_string(dictionary_value(actual, key)?)?;
        let expected = cf_string(dictionary_value(expected, key)?)?;
        if actual != expected {
            return Err(format!(
                "{key}: requested {expected:?}, read back {actual:?}"
            ));
        }
    }
    for key in numbers {
        let actual = cf_number(dictionary_value(actual, key)?)?;
        let expected = cf_number(dictionary_value(expected, key)?)?;
        if actual != expected {
            return Err(format!("{key}: requested {expected}, read back {actual}"));
        }
    }
    Ok(())
}

fn verify_composition(actual: CfRef, expected: CfRef) -> Result<(), String> {
    compare_fields(
        actual,
        expected,
        &["uid", "name", "master"],
        &["stacked", "private"],
    )?;
    let actual_members = dictionary_value(actual, "subdevices")?;
    let expected_members = dictionary_value(expected, "subdevices")?;
    for members in [actual_members, expected_members] {
        if unsafe { CFGetTypeID(members) != CFArrayGetTypeID() } {
            return Err("aggregate subdevices are not an array".into());
        }
    }
    let count = unsafe { CFArrayGetCount(expected_members) };
    if unsafe { CFArrayGetCount(actual_members) } != count {
        return Err("aggregate subdevice count differs from requested configuration".into());
    }
    for index in 0..count {
        compare_fields(
            unsafe { CFArrayGetValueAtIndex(actual_members, index) },
            unsafe { CFArrayGetValueAtIndex(expected_members, index) },
            &["uid"],
            &["latency-out", "drift", "drift quality"],
        )
        .map_err(|error| format!("subdevice {index}: {error}"))?;
    }
    Ok(())
}

fn verify_accepted_composition(id: u32, expected: &Cf) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let result = cf_property(id, fourcc(b"acom"))
            .and_then(|actual| verify_composition(actual.0, expected.0));
        match result {
            Ok(()) => return Ok(()),
            Err(error) if Instant::now() >= deadline => {
                return Err(format!(
                    "Core Audio configuration was not confirmed: {error}"
                ));
            }
            Err(_) => thread::sleep(Duration::from_millis(20)),
        }
    }
}

fn status(code: Status, action: &str) -> Result<(), String> {
    if code == 0 {
        return Ok(());
    }
    let bytes = code.to_be_bytes();
    let tag = if bytes.iter().all(u8::is_ascii_graphic) {
        format!(" ('{}')", String::from_utf8_lossy(&bytes))
    } else {
        String::new()
    };
    Err(format!("{action}: OSStatus {code}{tag}"))
}

fn property_bytes(id: u32, selector: u32, scope: u32) -> Result<Vec<u8>, String> {
    property_bytes_at(id, selector, scope, 0)
}

fn property_bytes_at(id: u32, selector: u32, scope: u32, element: u32) -> Result<Vec<u8>, String> {
    let address = Address {
        selector,
        scope,
        element,
    };
    let label = format!(
        "read {} on audio object {id}",
        String::from_utf8_lossy(&selector.to_be_bytes())
    );
    let mut size = 0u32;
    status(
        unsafe { AudioObjectGetPropertyDataSize(id, &address, 0, ptr::null(), &mut size) },
        &label,
    )?;
    if size as usize > MAX_PROPERTY_BYTES {
        return Err(format!("{label}: property exceeds size limit"));
    }
    let mut bytes = vec![0u8; size as usize];
    if size != 0 {
        status(
            unsafe {
                AudioObjectGetPropertyData(
                    id,
                    &address,
                    0,
                    ptr::null(),
                    &mut size,
                    bytes.as_mut_ptr().cast(),
                )
            },
            &label,
        )?;
        if size as usize > bytes.len() {
            return Err(format!("{label}: property grew while reading"));
        }
        bytes.truncate(size as usize);
    }
    Ok(bytes)
}

// Only called with plain C numbers, pointers, or structs whose bit patterns are all valid.
unsafe fn value<T: Copy>(id: u32, selector: u32, scope: u32) -> Result<T, String> {
    unsafe { value_at(id, selector, scope, 0) }
}

unsafe fn value_at<T: Copy>(id: u32, selector: u32, scope: u32, element: u32) -> Result<T, String> {
    let bytes = property_bytes_at(id, selector, scope, element)?;
    if bytes.len() != mem::size_of::<T>() {
        return Err(format!(
            "unexpected property size on audio object {id}: {}",
            bytes.len()
        ));
    }
    Ok(unsafe { bytes.as_ptr().cast::<T>().read_unaligned() })
}

pub fn read_u32(id: u32, selector: u32, scope: u32) -> Result<u32, String> {
    unsafe { value(id, selector, scope) }
}
pub fn read_f64(id: u32, selector: u32, scope: u32) -> Result<f64, String> {
    unsafe { value(id, selector, scope) }
}

fn ids(id: u32, selector: u32, scope: u32) -> Result<Vec<u32>, String> {
    let bytes = property_bytes(id, selector, scope)?;
    if bytes.len() % 4 != 0 {
        return Err("Core Audio returned a partial object ID".into());
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|bytes| u32::from_ne_bytes(bytes.try_into().unwrap()))
        .collect())
}

fn cf_property(id: u32, selector: u32) -> Result<Cf, String> {
    Cf::owned(unsafe { value::<CfRef>(id, selector, GLOBAL)? })
}

fn string_property(id: u32, selector: u32) -> Result<String, String> {
    cf_string(cf_property(id, selector)?.0)
}

fn channel_count(id: u32, scope: u32) -> Result<u32, String> {
    let bytes = property_bytes(id, fourcc(b"slay"), scope)?;
    if bytes.len() < 4 {
        return Err("Core Audio returned a truncated buffer list".into());
    }
    let count = u32::from_ne_bytes(bytes[..4].try_into().unwrap()) as usize;
    let offset = mem::offset_of!(AudioBufferList, buffers);
    let available = bytes.len().saturating_sub(offset) / mem::size_of::<AudioBuffer>();
    if count > available {
        return Err("Core Audio returned a truncated buffer array".into());
    }
    (0..count).try_fold(0u32, |total, index| {
        let buffer = unsafe {
            bytes
                .as_ptr()
                .add(offset + index * mem::size_of::<AudioBuffer>())
                .cast::<AudioBuffer>()
                .read_unaligned()
        };
        total
            .checked_add(buffer.channels)
            .ok_or_else(|| "channel count overflow".into())
    })
}

#[derive(Clone, Debug)]
pub struct Device {
    pub id: u32,
    pub uid: String,
    pub name: String,
    pub output_channels: u32,
    pub input_channels: u32,
    pub sample_rate: f64,
    pub transport: u32,
    pub is_aggregate: bool,
}

impl Device {
    pub fn is_physical_output(&self) -> bool {
        self.output_channels > 0 && !self.is_aggregate && physical_transport(self.transport)
    }
}

fn physical_transport(transport: u32) -> bool {
    [
        b"bltn", b"pci ", b"usb ", b"1394", b"blue", b"blea", b"hdmi", b"dprt", b"thun",
    ]
    .iter()
    .any(|value| fourcc(value) == transport)
}

fn device(id: u32) -> Result<Device, String> {
    device_with_uid(id, string_property(id, fourcc(b"uid "))?)
}

fn device_with_uid(id: u32, uid: String) -> Result<Device, String> {
    let transport = read_u32(id, fourcc(b"tran"), GLOBAL)?;
    Ok(Device {
        id,
        uid,
        name: string_property(id, fourcc(b"lnam"))?,
        output_channels: channel_count(id, OUTPUT)?,
        input_channels: channel_count(id, INPUT)?,
        sample_rate: read_f64(id, fourcc(b"nsrt"), GLOBAL)?,
        transport,
        is_aggregate: transport == fourcc(b"grup")
            || read_u32(id, fourcc(b"clas"), GLOBAL)? == fourcc(b"aagg"),
    })
}

pub fn devices() -> Result<Vec<Device>, String> {
    let mut devices = Vec::new();
    for id in ids(SYSTEM, fourcc(b"dev#"), GLOBAL)? {
        let Some(uid) = skip_disappeared(string_property(id, fourcc(b"uid ")), || {
            disappeared(id, None)
        })?
        else {
            continue;
        };
        let result = device_with_uid(id, uid.clone()).and_then(|device| {
            if string_property(id, fourcc(b"uid "))? != uid {
                return Err("audio object identity changed during enumeration".into());
            }
            Ok(device)
        });
        let result =
            result.map_err(|error| format!("inspect existing audio device {uid:?}: {error}"));
        if let Some(device) = skip_disappeared(result, || disappeared(id, Some(&uid)))? {
            devices.push(device);
        }
    }
    Ok(devices)
}

fn skip_disappeared<T>(
    result: Result<T, String>,
    vanished: impl FnOnce() -> Result<bool, String>,
) -> Result<Option<T>, String> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(_) if vanished()? => Ok(None),
        Err(error) => Err(error),
    }
}

fn disappeared(id: u32, original_uid: Option<&str>) -> Result<bool, String> {
    if !ids(SYSTEM, fourcc(b"dev#"), GLOBAL)?.contains(&id) {
        return Ok(true);
    }
    Ok(original_uid.is_some_and(|uid| {
        string_property(id, fourcc(b"uid ")).is_ok_and(|current| current != uid)
    }))
}

fn owner_pid(uid: &str) -> Option<i32> {
    let suffix = uid.strip_prefix("com.github.Byron.odaud.")?;
    let (kind, suffix) = suffix.split_once('.')?;
    if !matches!(kind, "run" | "probe") {
        return None;
    }
    let (pid, timestamp) = suffix.split_once('.')?;
    if !pid.bytes().all(|byte| byte.is_ascii_digit())
        || !timestamp.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let parsed_pid = pid.parse::<i32>().ok().filter(|pid| *pid > 0)?;
    let parsed_timestamp = timestamp.parse::<u128>().ok()?;
    if parsed_pid.to_string() != pid || parsed_timestamp.to_string() != timestamp {
        return None;
    }
    Some(parsed_pid)
}

pub fn is_owned_uid(uid: &str) -> bool {
    owner_pid(uid).is_some()
}

fn require_exited_owner(pid: i32) -> Result<(), String> {
    // Signal zero only checks existence/permission; it never signals or terminates the process.
    let result = unsafe { kill(pid, 0) };
    let errno = unsafe { *__error() };
    if result == -1 && errno == 3 {
        return Ok(());
    } // ESRCH, verified in the macOS SDK.
    if result == 0 {
        return Err(format!(
            "refusing cleanup: owner PID {pid} is still alive (or has been reused)"
        ));
    }
    Err(format!(
        "refusing cleanup: owner PID {pid} could not be checked (errno {errno})"
    ))
}

fn published_id_for_uid(uid: &str) -> Result<Option<u32>, String> {
    for id in ids(SYSTEM, fourcc(b"dev#"), GLOBAL)? {
        if string_property(id, fourcc(b"uid "))? == uid {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

fn wait_until_unpublished(uid: &str) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let result = published_id_for_uid(uid);
        if matches!(result, Ok(None)) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(match result {
                Err(error) => format!("cannot verify removal of {uid}: {error}"),
                _ => format!("aggregate {uid} is still listed 3 seconds after destruction"),
            });
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// Remove exactly one ODAUD aggregate only after its encoded owner is proven absent.
pub fn cleanup_orphan(uid: &str) -> Result<(), String> {
    let pid = owner_pid(uid).ok_or("cleanup requires an exact ODAUD run/probe UID with a positive PID and nanosecond timestamp")?;
    require_exited_owner(pid)?;
    let Some(id) = published_id_for_uid(uid)? else {
        return Ok(());
    };
    if read_u32(id, fourcc(b"clas"), GLOBAL)? != fourcc(b"aagg") {
        return Err("refusing cleanup: matching UID is not an aggregate device".into());
    }
    require_exited_owner(pid)?;
    if string_property(id, fourcc(b"uid "))? != uid {
        return Err("refusing cleanup: audio object identity changed".into());
    }
    status(
        unsafe { AudioHardwareDestroyAggregateDevice(id) },
        "destroy orphaned aggregate",
    )?;
    wait_until_unpublished(uid)
}

pub fn stream_formats(id: u32, input: bool) -> Result<Vec<(u32, StreamFormat)>, String> {
    ids(id, fourcc(b"stm#"), if input { INPUT } else { OUTPUT })?
        .into_iter()
        .map(|stream| Ok((stream, unsafe { value(stream, fourcc(b"sfmt"), GLOBAL)? })))
        .collect()
}

pub fn inspect(id: u32) -> Result<String, String> {
    let device = device(id)?;
    let mut result = format!(
        "{}\nuid = {}\nid = {}\nchannels = {} output, {} input\nsample_rate = {} Hz\n",
        device.name,
        device.uid,
        id,
        device.output_channels,
        device.input_channels,
        device.sample_rate
    );
    for (name, selector, scope) in [
        ("output_device_latency_frames", fourcc(b"ltnc"), OUTPUT),
        ("output_safety_offset_frames", fourcc(b"saft"), OUTPUT),
        ("buffer_frames", fourcc(b"fsiz"), GLOBAL),
        ("clock_domain", fourcc(b"clkd"), GLOBAL),
        ("can_be_default_output", fourcc(b"dflt"), OUTPUT),
        ("class", fourcc(b"clas"), GLOBAL),
        ("transport", fourcc(b"tran"), GLOBAL),
    ] {
        match read_u32(id, selector, scope) {
            Ok(value) => {
                let _ = writeln!(result, "{name} = {value}");
            }
            Err(error) => {
                let _ = writeln!(result, "{name} = unavailable ({error})");
            }
        }
    }
    for (stream, format) in stream_formats(id, false)? {
        let _ = writeln!(
            result,
            "output_stream {stream}: {format:?}; latency_frames={:?}",
            read_u32(stream, fourcc(b"ltnc"), GLOBAL)
        );
    }
    for (direction, scope, channels) in [
        ("input", INPUT, device.input_channels),
        ("output", OUTPUT, device.output_channels),
    ] {
        if channels == 0 {
            continue;
        }
        let mut found = false;
        for element in 0..=channels.min(32) {
            if let Ok(mute) = unsafe { value_at::<u32>(id, fourcc(b"mute"), scope, element) } {
                let _ = writeln!(result, "{direction}_mute[element={element}] = {mute}");
                found = true;
            }
            if let Ok(volume) = unsafe { value_at::<f32>(id, fourcc(b"volm"), scope, element) } {
                let _ = writeln!(
                    result,
                    "{direction}_volume_scalar[element={element}] = {volume}"
                );
                found = true;
            }
        }
        if !device.is_aggregate {
            if let Ok(mute) = read_u32(id, fourcc(b"appm"), scope) {
                let _ = writeln!(
                    result,
                    "{direction}_process_mute[current_process_only] = {mute}"
                );
                found = true;
            }
        }
        if !found {
            let _ = writeln!(result, "{direction}_mute_and_volume_controls = unavailable");
        }
    }
    if let Ok(composition) = cf_property(id, fourcc(b"acom")) {
        let _ = writeln!(
            result,
            "composition (native readback):\n{}",
            composition.description()?
        );
    }
    Ok(result)
}

fn validate_outputs(outputs: &[(String, f64)], master_uid: &str) -> Result<(), String> {
    if outputs.is_empty() || outputs.len() > 32 {
        return Err("aggregate requires 1..=32 outputs".into());
    }
    for (index, (uid, frames)) in outputs.iter().enumerate() {
        if uid.is_empty() || uid.len() > 4096 || uid.contains('\0') {
            return Err("invalid output UID".into());
        }
        if !frames.is_finite() || *frames < 0.0 || *frames > 3_840_000.0 {
            return Err(
                "native output latency must be finite and within 0..=3840000 frames".into(),
            );
        }
        if outputs[..index].iter().any(|(other, _)| other == uid) {
            return Err(format!("duplicate output UID: {uid}"));
        }
    }
    if !outputs.iter().any(|(uid, _)| uid == master_uid) {
        return Err("clock source must be an output member".into());
    }
    Ok(())
}

pub struct Aggregate {
    pub id: u32,
    uid: String,
    composition: Cf,
    outputs: Vec<(String, f64)>,
    master_uid: String,
}

fn aggregate_composition(
    name: &str,
    uid: &str,
    outputs: &[(String, f64)],
    master_uid: &str,
    stacked: bool,
    private: bool,
) -> Result<Cf, String> {
    let mut subdevices = Vec::with_capacity(outputs.len());
    for (member_uid, frames) in outputs {
        let mut member = Cf::dictionary()?;
        member.set("uid", &Cf::string(member_uid)?)?;
        member.set("latency-out", &Cf::number(*frames)?)?;
        member.set("drift", &Cf::integer(i32::from(member_uid != master_uid))?)?;
        member.set("drift quality", &Cf::integer(0x60)?)?;
        subdevices.push(member);
    }
    let mut composition = Cf::dictionary()?;
    composition.set("name", &Cf::string(name)?)?;
    composition.set("uid", &Cf::string(uid)?)?;
    composition.set("master", &Cf::string(master_uid)?)?;
    composition.set("stacked", &Cf::integer(i32::from(stacked))?)?;
    composition.set("private", &Cf::integer(i32::from(private))?)?;
    composition.set("subdevices", &Cf::array(&subdevices)?)?;
    Ok(composition)
}

impl Aggregate {
    pub fn create(
        name: &str,
        uid: &str,
        outputs: &[(String, f64)],
        master_uid: &str,
        stacked: bool,
        private: bool,
    ) -> Result<Self, String> {
        validate_outputs(outputs, master_uid)?;
        if name.is_empty()
            || uid.is_empty()
            || name.len() > 4096
            || uid.len() > 4096
            || name.contains('\0')
            || uid.contains('\0')
        {
            return Err("aggregate name and UID must be nonempty and bounded".into());
        }
        let available = devices()?;
        if available.iter().any(|device| device.uid == uid) {
            return Err(format!("aggregate UID already exists: {uid}"));
        }
        for (uid, _) in outputs {
            if !available.iter().any(|device| {
                device.uid == *uid && device.output_channels >= 2 && device.is_physical_output()
            }) {
                return Err(format!(
                    "known physical stereo output is not available: {uid}"
                ));
            }
        }
        let composition = aggregate_composition(name, uid, outputs, master_uid, stacked, private)?;
        let mut id = 0;
        status(
            unsafe { AudioHardwareCreateAggregateDevice(composition.0, &mut id) },
            "create aggregate",
        )?;
        if id == 0 {
            return Err("Core Audio created no aggregate object".into());
        }
        let aggregate = Self {
            id,
            uid: uid.into(),
            composition,
            outputs: outputs.to_vec(),
            master_uid: master_uid.into(),
        };
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if read_u32(id, fourcc(b"livn"), GLOBAL) == Ok(1)
                && channel_count(id, OUTPUT).is_ok_and(|count| count >= 2)
            {
                verify_accepted_composition(id, &aggregate.composition)?;
                return Ok(aggregate);
            }
            if Instant::now() >= deadline {
                return Err("aggregate did not become ready within 3 seconds".into());
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// A failed native write or readback may leave partial changes; callers must stop the aggregate.
    pub fn set_latencies(&mut self, outputs: &[(String, f64)]) -> Result<(), String> {
        validate_outputs(outputs, &self.master_uid)?;
        if self.id == 0 {
            return Err("aggregate was destroyed".into());
        }
        if string_property(self.id, fourcc(b"uid "))? != self.uid {
            return Err("aggregate object identity changed".into());
        }
        if outputs.len() != self.outputs.len()
            || self
                .outputs
                .iter()
                .any(|(uid, _)| !outputs.iter().any(|(other, _)| uid == other))
        {
            return Err("live update cannot change output membership".into());
        }
        let current = cf_property(self.id, fourcc(b"acom"))?;
        verify_composition(current.0, self.composition.0)
            .map_err(|error| format!("aggregate changed outside the application: {error}"))?;
        if unsafe { CFGetTypeID(current.0) != CFDictionaryGetTypeID() } {
            return Err("aggregate composition is not a dictionary".into());
        }
        let mut next =
            Cf::owned(unsafe { CFDictionaryCreateMutableCopy(ptr::null(), 0, current.0) })?;
        let key = Cf::string("subdevices")?;
        let members = unsafe { CFDictionaryGetValue(current.0, key.0) };
        if members.is_null() || unsafe { CFGetTypeID(members) != CFArrayGetTypeID() } {
            return Err("aggregate has no subdevice array".into());
        }
        let count = unsafe { CFArrayGetCount(members) };
        if count != outputs.len() as isize {
            return Err("aggregate membership changed outside this process".into());
        }
        let uid_key = Cf::string("uid")?;
        let mut changed_members = Vec::with_capacity(outputs.len());
        let mut seen = Vec::with_capacity(outputs.len());
        for index in 0..count {
            let member = unsafe { CFArrayGetValueAtIndex(members, index) };
            if member.is_null() || unsafe { CFGetTypeID(member) != CFDictionaryGetTypeID() } {
                return Err("aggregate subdevice is not a dictionary".into());
            }
            let uid = cf_string(unsafe { CFDictionaryGetValue(member, uid_key.0) })?;
            let (_, frames) = outputs
                .iter()
                .find(|(candidate, _)| *candidate == uid)
                .ok_or_else(|| format!("unowned aggregate member: {uid}"))?;
            if seen.contains(&uid) {
                return Err("duplicate native aggregate member".into());
            }
            seen.push(uid);
            let mut changed =
                Cf::owned(unsafe { CFDictionaryCreateMutableCopy(ptr::null(), 0, member) })?;
            changed.set("latency-out", &Cf::number(*frames)?)?;
            changed_members.push(changed);
        }
        next.set("subdevices", &Cf::array(&changed_members)?)?;
        let address = Address {
            selector: fourcc(b"acom"),
            scope: GLOBAL,
            element: 0,
        };
        let mut settable = 0;
        status(
            unsafe { AudioObjectIsPropertySettable(self.id, &address, &mut settable) },
            "query live aggregate composition support",
        )?;
        if settable == 0 {
            return Err("Core Audio does not permit live aggregate composition changes".into());
        }
        status(
            unsafe {
                AudioObjectSetPropertyData(
                    self.id,
                    &address,
                    0,
                    ptr::null(),
                    mem::size_of::<CfRef>() as u32,
                    ptr::from_ref(&next.0).cast(),
                )
            },
            "update aggregate composition",
        )?;
        verify_accepted_composition(self.id, &next)?;
        self.composition = next;
        self.outputs = outputs.to_vec();
        Ok(())
    }

    pub fn destroy(&mut self) -> Result<(), String> {
        if self.id == 0 {
            return Ok(());
        }
        let id = self.id;
        match string_property(id, fourcc(b"uid ")) {
            Ok(uid) if uid == self.uid => {}
            Ok(_) => {
                self.id = 0;
                return Err("refusing to destroy an audio object whose UID changed".into());
            }
            Err(error) => {
                if !ids(SYSTEM, fourcc(b"dev#"), GLOBAL)?.contains(&id) {
                    self.id = 0;
                    return Ok(());
                }
                return Err(error);
            }
        }
        status(
            unsafe { AudioHardwareDestroyAggregateDevice(id) },
            "destroy aggregate",
        )?;
        // Once destruction is accepted, never issue another destroy against a potentially reused ID.
        self.id = 0;
        wait_until_unpublished(&self.uid)
    }
}

impl Drop for Aggregate {
    fn drop(&mut self) {
        let _ = self.destroy();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumeration_skips_confirmed_disappearance_but_keeps_live_device_errors() {
        let refreshed_ids = [1u32];
        let mut enumerated = Vec::new();
        for (id, read) in [(1, Ok("selected")), (2, Err("device disconnected".into()))] {
            if let Some(device) =
                skip_disappeared(read, || Ok(!refreshed_ids.contains(&id))).unwrap()
            {
                enumerated.push(device);
            }
        }
        assert_eq!(enumerated, ["selected"]);
        assert_eq!(
            skip_disappeared::<()>(Err("bad live property".into()), || Ok(false)),
            Err("bad live property".into())
        );
        assert!(
            skip_disappeared::<()>(Err("bad property".into()), || Err(
                "cannot confirm disappearance".into()
            ))
            .is_err()
        );
    }

    #[test]
    fn composition_readback_checks_shape_drift_and_exact_delays() {
        let outputs = [("a".into(), 0.0), ("b".into(), 24_000.0)];
        let build = || aggregate_composition("ODAUD", "owned", &outputs, "a", true, false).unwrap();
        let expected = build();
        let mut actual = build();
        actual
            .set("extra-native-metadata", &Cf::integer(6).unwrap())
            .unwrap();
        assert!(verify_composition(actual.0, expected.0).is_ok());
        for (key, changed) in [
            ("latency-out", 23_999.0),
            ("drift", 0.0),
            ("drift quality", 0.0),
        ] {
            let actual = build();
            let members = dictionary_value(actual.0, "subdevices").unwrap();
            let member = unsafe { CFArrayGetValueAtIndex(members, 1) };
            let key_object = Cf::string(key).unwrap();
            let number = Cf::number(changed).unwrap();
            unsafe { CFDictionarySetValue(member, key_object.0, number.0) };
            assert!(
                verify_composition(actual.0, expected.0)
                    .unwrap_err()
                    .contains(key)
            );
        }
        let reversed = aggregate_composition(
            "ODAUD",
            "owned",
            &[("b".into(), 24_000.0), ("a".into(), 0.0)],
            "a",
            true,
            false,
        )
        .unwrap();
        assert!(verify_composition(reversed.0, expected.0).is_err());
        let mut private = build();
        private.set("private", &Cf::integer(1).unwrap()).unwrap();
        assert!(
            verify_composition(private.0, expected.0)
                .unwrap_err()
                .contains("private")
        );
        assert!(cf_number(Cf::string("not a number").unwrap().0).is_err());
        assert!(cf_number(Cf::number(f64::NAN).unwrap().0).is_err());
    }

    #[test]
    fn only_exact_owned_uids_can_reach_orphan_cleanup() {
        assert_eq!(
            owner_pid("com.github.Byron.odaud.probe.53514.1791961942404340000"),
            Some(53514)
        );
        assert_eq!(
            owner_pid("com.github.Byron.odaud.run.1.340282366920938463463374607431768211455"),
            Some(1)
        );
        for uid in [
            "BuiltInSpeakerDevice",
            "com.github.Byron.odaud",
            "com.github.Byron.odaud.other.1.2",
            "com.github.Byron.odaud.run.0.1",
            "com.github.Byron.odaud.run.-1.2",
            "com.github.Byron.odaud.run.+1.2",
            "com.github.Byron.odaud.run.01.2",
            "com.github.Byron.odaud.run.2147483648.2",
            "com.github.Byron.odaud.run.1.",
            "com.github.Byron.odaud.run.1.2.3",
            "com.github.Byron.odaud.run.1.+2",
            "com.github.Byron.odaud.run.1.-2",
            "com.github.Byron.odaud.run.1.02",
            "com.github.Byron.odaud.run.1.340282366920938463463374607431768211456",
        ] {
            assert!(!is_owned_uid(uid), "accepted {uid}");
            assert!(cleanup_orphan(uid).is_err(), "cleanup accepted {uid}");
        }
    }

    #[test]
    fn a_live_owner_cannot_be_cleaned_up() {
        let uid = format!("com.github.Byron.odaud.run.{}.1", std::process::id());
        assert!(cleanup_orphan(&uid).unwrap_err().contains("still alive"));
    }

    #[test]
    fn validates_native_latency_before_device_management() {
        assert!(validate_outputs(&[("a".into(), 0.0), ("b".into(), 4800.0)], "a").is_ok());
        for delay in [-1.0, f64::NAN, f64::INFINITY, 3_840_001.0] {
            assert!(validate_outputs(&[("a".into(), delay)], "a").is_err());
        }
        assert!(validate_outputs(&[("a".into(), 0.0), ("a".into(), 1.0)], "a").is_err());
        assert!(validate_outputs(&[("a".into(), 0.0)], "b").is_err());
        assert!(validate_outputs(&[], "a").is_err());
    }

    #[test]
    fn physical_members_exclude_virtual_aggregate_and_remote_transports() {
        for transport in [b"bltn", b"usb ", b"hdmi", b"dprt", b"thun", b"blue"] {
            assert!(physical_transport(fourcc(transport)));
        }
        for transport in [b"grup", b"virt", b"airp", b"rscr", b"rstr", b"????"] {
            assert!(!physical_transport(fourcc(transport)));
        }
    }

    #[test]
    fn ffi_layout_matches_macos_sdk() {
        assert_eq!(mem::size_of::<Address>(), 12);
        assert_eq!(mem::size_of::<StreamFormat>(), 40);
        assert_eq!(mem::size_of::<AudioBuffer>(), 16);
        assert_eq!(mem::offset_of!(AudioBufferList, buffers), 8);
        assert_eq!(fourcc(b"glob"), 0x676c6f62);
    }

    #[test]
    fn cf_collections_retain_strings_and_copy_without_mutating_original() {
        let mut dictionary = Cf::dictionary().unwrap();
        dictionary
            .set("name", &Cf::string("ODAUD – stereo").unwrap())
            .unwrap();
        dictionary
            .set("latency-out", &Cf::number(2400.0).unwrap())
            .unwrap();
        let array = Cf::array(&[dictionary]).unwrap();
        let original = unsafe { CFArrayGetValueAtIndex(array.0, 0) };
        let mut changed =
            Cf::owned(unsafe { CFDictionaryCreateMutableCopy(ptr::null(), 0, original) }).unwrap();
        changed
            .set("name", &Cf::string("Changed").unwrap())
            .unwrap();
        let key = Cf::string("name").unwrap();
        assert_eq!(
            cf_string(unsafe { CFDictionaryGetValue(original, key.0) }).unwrap(),
            "ODAUD – stereo"
        );
        assert_eq!(
            cf_string(unsafe { CFDictionaryGetValue(changed.0, key.0) }).unwrap(),
            "Changed"
        );
    }
}
