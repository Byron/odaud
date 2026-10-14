# ODAUD

ODAUD creates one selectable macOS stereo output that mirrors audio to physical
outputs with independently configurable relative delays. It runs in the
foreground, reads TOML configuration, and removes its output on a clean stop.
Alternatively, `persist` creates the output and leaves it available after exiting.
The current interface is a Rust CLI; a GUI is deferred.

The agreed implementation uses Core Audio's native aggregate engine for
playback, clock-drift compensation, and delay compensation. ODAUD owns the
aggregate's creation, configuration, and teardown. It installs no driver,
privileged helper, launch service, or daemon. Select its output explicitly in
macOS or your player. Only `test --sound real-audio` temporarily changes the
default output, so you can test with audio from another app.

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

## Keep the device after exiting

```sh
./target/release/odaud persist examples/bose.toml
./target/release/odaud persist examples/bose.toml --force
./target/release/odaud cleanup com.github.Byron.odaud.persist
```

`persist` validates the configuration, creates the same mirrored output as
`run`, and exits without removing it. Core Audio continues to provide playback,
delays, and drift compensation; no ODAUD process or extra service stays running.
Select the device explicitly in macOS or your player.

There is **one persistent ODAUD device**, identified by the stable UID
`com.github.Byron.odaud.persist`, regardless of configuration path or device
name. If it already exists, `persist` fails unless `--force` is supplied.
`--force` validates the new configuration and connected outputs, removes the
existing device, and recreates it. Replacement interrupts playback and may
require selecting the device again. If recreation fails, the old device is
already gone. Temporary devices from `run` and `test` are separate.

Use `persist --force` to apply configuration edits; there is no file reload or
ODAUD monitoring of disconnection, sleep, or sample-rate changes after exit.
Offsets remain in frames at the rate used during creation, so reapply the
configuration after changing sample rates. Run persistence and cleanup commands
sequentially. `cleanup` removes the persistent device explicitly.
Survival across reboot, logout, or an audio-server restart is unverified.

## Listen to the configuration

```sh
./target/release/odaud test examples/bose.toml
./target/release/odaud test examples/bose.toml --seconds 120
./target/release/odaud test examples/bose.toml --sound pulse --seconds 120
./target/release/odaud test examples/bose.toml --latency-check
./target/release/odaud test examples/bose.toml --sound real-audio --latency-check
```

`test` creates the configured device and emits a quiet 20 ms chirp each second
through all enabled outputs, including their configured delays. It needs no
manual output selection. Generated sounds do not change the system default.
It stops after 30 seconds by default and removes the device; Ctrl-C stops it early.

`--sound pulse` plays a steady four-beat-per-second pattern instead of chirps.
Each beat is a quiet 60 ms tone, with a higher accent each second and a distinct
accent at the start of its eight-second phrase. Timing differences can make
these predictable attacks sound doubled. Both sounds peak at most at -26 dBFS;
speaker volume and listening position still matter.

While listening, edit the delays and send SIGHUP to the printed PID to hear
the changes. The same configuration validation and shutdown handling as
`run` apply. Choose a duration at least one second longer than the largest
configured delay. Use `odaud test --help` or `odaud --help` for generated CLI
help.

### Use audio from another app

`--sound real-audio` selects the test device as the macOS default output;
play music, speech, or any other sound in your preferred player. ODAUD generates
no sound and does not capture or replay the player's audio. The player must use
the system default output; an app pinned to a specific device will not follow
the changes. Playback keeps the player's volume, without the quiet level limit
of the generated sounds. The separate macOS output for alerts is unchanged.

With `--latency-check`, the default output is the first enabled physical output
while choosing a device. Enter switches it to the aggregate for comparison;
Esc switches it back to the reference. A player may pause, reopen its output,
or introduce a gap when the default changes. Without `--latency-check`, audio
uses the aggregate for the test duration, which defaults to 30 seconds.

On a clean stop, ODAUD restores the previous default output if its selected
output is still current. If you select another output yourself, ODAUD stops
and preserves that selection. It checks every 250 ms and before switching or
restoring; a change away and back between checks can be missed, and a change
concurrent with a native routing write can race. The original device is found
by UID; restoration reports an error if it is no longer available.

## Adjust delays interactively

`test CONFIG.toml --latency-check` opens inline terminal controls without an
alternate screen. It defaults to the predictable pulse pattern. Use
`--sound chirp` for chirps or `--sound real-audio` for another app's audio.
The check runs until you quit, with a two-hour limit; `--seconds` sets a shorter
limit for the whole session.
It requires a terminal and at least two enabled outputs in the configuration.

While choosing a device, only the **first enabled physical output** plays the
reference sound, without the configured aggregate delay. Up/Down highlights
an output; Enter selects its delay for adjustment and starts playback through
all configured outputs. The aggregate stays available throughout the session.

| Key while adjusting | Action |
| --- | --- |
| Up / Down | Increase / decrease the selected delay by 1 ms |
| Right / Left | Increase / decrease it by 10 ms |
| Esc | Return to device selection and single-speaker reference playback |
| s | Save the current delays to the configuration file |
| q or Ctrl-C | Stop, remove the test device, and print the final values |

Delay changes apply immediately through Core Audio, bounded to 0–5000 ms.
For generated sounds, switching between reference and comparison stops one
source before starting the other; it can introduce a brief gap. The displayed
delay is the requested compensation, not a measurement of total acoustic
latency. Listen from your usual position and adjust the earlier output until
the attacks sound together.

Changes stay in memory until you press **s**, which works in either mode.
Saving preserves comments, other configuration fields, and file permissions.
It refuses symbolic/hard links and files changed since the last load or save;
avoid editing the same file concurrently with saving. SIGHUP reloads the file
and replaces unsaved adjustments. Final values are printed on exit whether
or not you saved, including when the session stops on an error or signal.

The check uses its own temporary device. After saving, use `run CONFIG.toml`
or `persist CONFIG.toml --force` to use the calibrated settings normally.

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

For `run` and `test`, the application stops and removes the aggregate when it detects output loss,
a sample-rate change, or system sleep. Run it again after fixing the device
configuration or waking. Physical Bose unplug/reconnect and a subsequent
restart were tested. Actual sleep/wake and sample-rate-change checks remain pending.

## Align the speakers

1. Start with both delays at zero. Run `odaud test CONFIG.toml --latency-check`
   and listen to the single-speaker reference from your usual position.
2. Select the earlier speaker, press Enter, and increase **that output's** delay
   with the arrow keys while both outputs play.
3. Adjust in small steps until the two attacks sound as close as possible.
   If the order reverses, reduce the added delay.
4. Keep one output near zero where practical; adding the same delay to both
   does not improve relative alignment.
5. Press s to save. Recheck after moving speakers, changing connections, or
   changing the listening position. The saved file supplies the same settings next run.

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
No process cleanup can run after that signal. In `--sound real-audio` mode,
the default output may also remain changed; select your preferred output
manually. Use the orphan's exact ODAUD UID with
`cleanup`; cleanup refuses a temporary device whose owning process is still alive.
The exact persistent UID can be cleaned up without an owning process.
Normal shutdown of `run` and `test` was verified to remove the device from a separate process's
enumeration. For `--sound real-audio`, clean shutdown attempts guarded default
restoration before removing the device. Other modes leave default selection
to the user; macOS handles fallback from a disappearing selected device.

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
parsing uses Clap derives; configuration uses Serde, TOML, and toml_edit.
Crossterm supplies keyboard input and terminal control for the inline editor.
[Dependency evidence](docs/dependencies.md) records every
locked version, license, maintenance check, and applicable security advisory.
