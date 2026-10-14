# Native aggregate feasibility record

## Agreed platform approach

The initial research question was whether a temporary, selectable output could
intercept other applications' audio in a Rust processing callback without an
installed driver. A process tap captures audio; it does not by itself prove
that selectable-output requirement.

The user subsequently approved testing a native Core Audio aggregate as a
solution if it provides one temporary selectable output, drift compensation,
and independent relative delays, even when Apple's engine handles playback
without an application processing callback. That is the implemented scope.
Other applications' audio has **not** been shown reaching an ODAUD Rust DSP
callback, and this project does not claim that a hardware aggregate provides
arbitrary interception. Its diagnostic Rust callback generates its own test
source.

The later `persist CONFIG.toml` command explicitly keeps the same public
aggregate after the creating process exits. It uses one stable persistent UID;
`--force` replaces that device and `cleanup` removes it. No ODAUD process remains
to monitor hardware or apply configuration changes. This mode is separate from
the managed lifetime of `run` and `test`.

The implementation calls `AudioHardwareCreateAggregateDevice` with public
visibility and physical subdevices. ODAUD configures native drift compensation
and `kAudioSubDeviceExtraOutputLatencyKey` (`latency-out`) in each subdevice's
description. Live updates set `kAudioAggregateDevicePropertyComposition`
(`acom`) with new offsets and the same membership. Creation, reconfiguration,
and destruction run outside audio callbacks. Normal routing uses no process
tap, microphone, capture-and-replay loop, or custom resampler.

Relevant Apple interfaces:

- [AudioHardwareCreateAggregateDevice](https://developer.apple.com/documentation/coreaudio/audiohardwarecreateaggregatedevice(_:_:))
- [AudioHardwareDestroyAggregateDevice](https://developer.apple.com/documentation/coreaudio/audiohardwaredestroyaggregatedevice(_:))
- [AudioHardwareCreateProcessTap](https://developer.apple.com/documentation/coreaudio/audiohardwarecreateprocesstap(_:_:))
- [CATapDescription](https://developer.apple.com/documentation/coreaudio/catapdescription)

The installed Core Audio SDK documents aggregate destruction as asynchronous.
It describes extra subdevice latency as sample frames added to or subtracted
from latency compensation. Neither a successful property write nor a matching
readback alone proves audible delay.

## Hardware observations

Tests took place on **2026-10-14**, on **macOS 27.0.1** as the logged-in user, with **Bose SoundLink
Home USB** and **built-in MacBook speakers**, both at **48,000 Hz**. No driver,
elevation, privileged helper, reboot, or separately installed service was used.

| Check | Observation |
| --- | --- |
| Visibility to another application | Audio MIDI Setup visibly listed `ODAUD Feasibility`, with 0 inputs and 2 outputs. |
| Separate-client audio flow | An independent source process selected the aggregate; the user heard the stereo source on both physical outputs. |
| Stream composition | On this OS, `stacked=0` exposed 4 output channels; `stacked=1` exposed the required 2-channel mirrored output. The application uses the observed working setting and checks the resulting format. |
| Live Bose delay | A SIGHUP update from 0 to a requested 500 ms made built-in speakers sound first and Bose second. Returning to zero restored the previous relative timing. |
| Live built-in delay | A requested 500 ms on built-in speakers made Bose sound first and built-in speakers second. |
| Device continuity during delay reload | Reloads retained the aggregate UID and output format. |
| Default routing | Default and system-default output remained the built-in speakers during the experiment; ODAUD performed no default-output writes. |
| Clean shutdown | After normal termination, a separate enumerating process no longer found the public device. |
| Abnormal termination | After SIGKILL, the public aggregate remained. Explicit orphan cleanup is required. |
| Orphan recovery | Cleanup removed the exact crashed probe UID. Cleanup of a live ODAUD UID was refused. |
| Configuration reload | An unknown TOML field was rejected without removing the device; a valid 12.5 ms reload became 600 native frames at 48 kHz on the same UID. |
| USB disconnection | Physical Bose unplug stopped ODAUD and removed its aggregate. The Bose was visible after reconnection, and the release build successfully restarted. |
| Release build | Startup and a 25 ms reload passed native composition readback. A name change was rejected during reload. SIGTERM removed the device, and a separate enumeration confirmed no ODAUD devices remained. |
| Configuration chirp command | `test examples/bose.toml --seconds 5` sent 240,000 frames through the stereo aggregate and removed it automatically. A second test accepted a 50 ms SIGHUP edit and stopped cleanly on SIGINT after joining its playback worker. |
| Pulse test sound | `test examples/bose.toml --sound pulse --seconds 3` submitted 144,000 frames of the accented four-beat-per-second pattern through the configured aggregate and removed the device automatically. |
| Persistent device after exit | `persist` exited successfully. Separate enumeration and inspection found the public stereo device at 48 kHz with the requested 2,400-frame built-in offset. A separate chirp client then submitted 144,000 frames through it. This check did not include a new acoustic measurement or listening confirmation. |
| Persistent replacement and removal | A repeat without `--force` failed; invalid delays and an unavailable output with `--force` left the existing device intact. Valid replacement changed the name and built-in offset to 3,600 frames. Explicit cleanup removed the device; default and system-default output IDs stayed 73 throughout. Core Audio reused the numeric device ID across recreation. |

The installed SDK's prose for `kAudioAggregateDeviceIsStackedKey` describes
zero as mirrored output. That description did not match the observed channel
counts on this machine. The observations above, including actual listening,
are the basis for using one, rather than an assumption from the key's name.
Behavior on other macOS versions has not been established.

The user explicitly accepted listening as the current hardware validation and
relaxed microphone-based measurement. A three-second microphone capture
returned 144,000 frames of zeros at 48 kHz, with no permission prompt observed
while the laptop lid was open. This did **not** validate microphone capture,
permission handling, or acoustic latency measurement. No explanation for the
zero signal was established.

## What the observations establish

A public runtime aggregate can be selected by a separate audio client and
mirror its stereo source to these two physical outputs. Native per-output
latency controls audibly change relative timing in the expected direction and
can be reset without replacing the device. The app explicitly destroys the
aggregate on a clean stop in `run` and `test`. `persist` intentionally releases
its Rust management state without destroying the public aggregate. Retention
after process exit was tested; reboot, logout, and audio-server restart were not.

The test establishes direction and a noticeable relative shift, **not a
measured 500 ms shift**. Delay requests are rounded to sample frames. Reported
device and stream latency, safety offsets, buffer sizes, and native clock
compensation do not account for every downstream or acoustic delay. Listening
or measurement at the intended seat is still needed for alignment.

Core Audio handles the realtime playback engine and separate device clocks;
there is no application delay ring buffer whose impulse accuracy could stand
in for testing native behavior. The application enables drift compensation on
non-reference outputs, but sustained clock stability is not yet measured.

## Reproduce and extend the experiments

Build with `cargo build --release --locked`. Use `devices` and `inspect UID` to
record exact physical UIDs, formats, current rates, and reported latency data.
The normal `run examples/bose.toml` command and manual listening procedure are
described in [the README](../README.md).

For a one-command listening test, use `odaud test examples/bose.toml --seconds 30`.
It creates the configured aggregate, emits chirps through it in the same process,
accepts SIGHUP delay edits, and tears down when the timer ends or Ctrl-C is pressed.
The separate-client probes below remain available for platform experiments.

Experimental commands retained for a developer are:

```text
odaud probe-native STACKED SECONDS RELOAD_FILE_OR_DASH UID DELAY_MS [UID DELAY_MS ...]
odaud probe-play UID SECONDS
odaud probe-capture UID SECONDS OUTPUT.wav
```

`probe-native` prints its temporary UID and process ID. Use that UID in a
**separate** `probe-play` process, which emits quiet repeating synthetic
chirps. A reload file contains one whitespace-separated delay in milliseconds
per selected physical output; SIGHUP applies it. Supply `-` when no reload file
is needed. Device selection and enumeration must be checked from outside the
creating process. `probe-capture` is only a diagnostic for authorized test
measurements; normal routing does not capture or record audio.

To verify abnormal cleanup, retain the exact printed UID, terminate only the
known test process, enumerate with `devices --all`, and use `cleanup UID` on
the orphan. Cleanup is explicit and refuses a live owning process. Never
infer crash cleanup from a normal destructor running.

Outstanding hardware validation:

- Measure requested versus actual delay with impulses, including zero, small
  offsets, boundaries, and stereo isolation, using a working capture setup.
- Check live changes for clicks, gaps, or other discontinuities; unchanged
  UID and format do not prove uninterrupted playback.
- Run at least an hour with Bose and built-in outputs to evaluate drift and
  native compensation behavior under sustained playback.
- Exercise sample-rate changes and sleep/wake. USB unplug/reconnect and restart
  were tested; the policy stops and removes the aggregate, requiring restart.
- Verify permission-denial and grant behavior for the diagnostic capture path
  if that path is used for future measurements.
- Recheck visibility, stream composition, and cleanup on other macOS versions
  before claiming support for them.

All 33 automated tests pass, along with Clippy with warnings denied and the
locked release build. Tests cover the implementation's own parsing, bounds,
configuration transitions, control inputs, and diagnostic buffer handling.
Compilation and those tests do not complete the outstanding hardware checks.
