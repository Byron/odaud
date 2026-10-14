# ODAUD

ODAUD creates one selectable macOS stereo output that mirrors audio to physical
outputs with independently configurable relative delays. It runs in the
foreground, reads TOML configuration, and removes its output on a clean stop.
The current interface is a Rust CLI; a GUI is deferred.

The agreed implementation uses Core Audio's native aggregate engine for
playback, clock-drift compensation, and delay compensation. ODAUD owns the
aggregate's creation, configuration, and teardown. It installs no driver,
privileged helper, launch service, or daemon, and never writes the default
output settings. Select its output explicitly in macOS or your player.

On macOS 27.0.1, Bose SoundLink Home USB and built-in MacBook speakers at 48 kHz
played a separate client's stereo signal together. Listening confirmed the
direction of live delay changes and their reversal. Exact delay accuracy,
click-free changes, and long-session clock stability remain unmeasured. See
[the feasibility record](docs/feasibility.md) for observed results and limits.

## Build and run

Prerequisites: macOS, Rust 1.85 or later, and Apple's Xcode Command Line Tools.
Only macOS 27.0.1 has been hardware-tested; no earlier supported OS version has
been established.

```sh
cargo build --release --locked
./target/release/odaud devices
./target/release/odaud check examples/bose.toml
./target/release/odaud run examples/bose.toml
```

The example contains the UIDs observed on the test machine. Replace them with
the exact UIDs from `devices` if yours differ. `check` validates configuration
and connected outputs without creating a device. `run` prints its process ID,
aggregate UID, sample rate, clock reference, and requested offsets, then stays
in the foreground.

Choose **ODAUD** in your application's output selector or the macOS sound
controls. Changing the system default there is your explicit action. Press
Ctrl-C in the running terminal to stop. SIGTERM also requests a clean stop.

## Configuration

```toml
name = "ODAUD"

[[outputs]]
uid = "BuiltInSpeakerDevice"
enabled = true
delay_ms = 0

[[outputs]]
uid = "AppleUSBAudioEngine:Actions:Bose SoundLink Home USB:ACTIONS-DEV-SN0:2"
enabled = true
delay_ms = 0
```

`name` defaults to `ODAUD`, `enabled` to `true`, and `delay_ms` to `0`. Between
one and 32 outputs must be enabled. UIDs must be unique, including disabled entries.
Delays must be finite and between 0 and 5000 milliseconds. Unknown fields and
configuration files larger than 1 MiB are rejected.

Enabled devices must be physical stereo outputs with the same current nominal
sample rate, from 8 to 192 kHz. Use Audio MIDI Setup to set matching rates
before starting. The first enabled output is the clock reference; Core Audio
drift compensation is enabled for the remaining outputs. Matching nominal
rates alone does not synchronize clocks.

Offsets are converted to whole frames at that rate and passed to Core Audio's
native latency-compensation controls. Positive values made the corresponding
speaker later in the tested pair. These controls provide relative alignment;
the displayed value is not a measured end-to-end acoustic latency. Hardware
latency, safety offsets, buffering, and listening distance still contribute.

Edit delays and send SIGHUP to the **actual PID printed by `run`**:

```sh
kill -HUP PID
```

Replace `PID` with that number. Successful reloads retain the same aggregate
UID. Changing the name, enabled outputs, or their order requires a stop and
restart. Invalid reloads are reported; the previous accepted configuration
remains in use. A failed native update stops the device, because a partially
applied Core Audio change cannot safely be treated as the previous configuration.

The application stops and removes the aggregate when it detects output loss,
a sample-rate change, or system sleep. Run it again after fixing the device
configuration or waking. Physical Bose unplug/reconnect and a subsequent
restart were tested. Actual sleep/wake and sample-rate-change checks remain pending.

## Align the speakers

1. Start with both delays at zero. Play a quiet, repeating click or short
   percussive sound through ODAUD and listen from your usual position.
2. Identify the earlier speaker. Increase **that output's** `delay_ms` in the
   TOML file, then send SIGHUP to the printed process ID.
3. Adjust in small steps until the two attacks sound as close as possible.
   If the order reverses, reduce the added delay.
4. Keep one output near zero where practical; adding the same delay to both
   does not improve relative alignment.
5. Recheck after moving speakers, changing connections, or changing the
   listening position. The saved file supplies the same settings next run.

## Inspect devices and recover after a crash

```sh
./target/release/odaud devices
./target/release/odaud devices --all
./target/release/odaud inspect 'EXACT_DEVICE_UID'
./target/release/odaud cleanup 'EXACT_ORPHANED_ODAUD_UID'
```

`devices` lists physical outputs. `devices --all` also shows aggregates and
other Core Audio devices. `inspect` reports formats and available latency,
safety-offset, buffer, and clock information.

**SIGKILL leaves the public aggregate behind on the tested macOS version.**
No process cleanup can run after that signal. Use its exact ODAUD UID with
`cleanup`; cleanup refuses a device whose owning process is still alive.
Normal shutdown was verified to remove the device from a separate process's
enumeration. Because ODAUD never changes defaults, it has no application-owned
default change to restore; macOS handles any fallback from a disappearing
device selected by the user.

## Validation and dependencies

```sh
cargo test --locked
```

Tests cover configuration boundaries, reload compatibility, native control
input validation, and diagnostic callback buffer handling. They do not prove
Core Audio's acoustic delay accuracy. Diagnostic source/capture commands are
documented in [the feasibility record](docs/feasibility.md); they are test
tools, not recording or process-capture features of normal routing.

The audio path uses Apple's native frameworks through Rust FFI. Argument
parsing uses Clap derives; configuration uses Serde and TOML.
[Dependency evidence](docs/dependencies.md) records every
locked version, license, maintenance check, and applicable security advisory.
