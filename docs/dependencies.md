# Dependency evidence

Checked on **2026-10-14** against the live crates.io API, published crate
manifests, upstream GitHub repositories, and the RustSec advisory database.

## Adopted dependencies

The audio engine uses **no third-party audio dependencies**. It uses Rust's
standard library and narrowly scoped Rust FFI to Apple's installed Core Audio,
Core Foundation, and IOKit frameworks. These are platform
facilities, not vendored implementations of audio primitives. No graphical UI
toolkit, resampler, ring-buffer crate, plugin host, or asynchronous runtime is
used. Clap handles argument parsing, Serde and TOML parse configuration,
Crossterm handles inline terminal interaction, and TOML edit preserves comments
when saving explicitly requested delay changes, as detailed below.

The native framework definitions must be checked against the installed macOS
SDK. Rust owns callback state and buffers; Apple's frameworks retain their
platform implementation and licensing. Build tools such as Cargo, rustc, and
Apple's linker are build prerequisites, not application runtime dependencies.

`Cargo.lock` records forty-seven third-party packages. Forty-one occur in the
active macOS normal/build graph; six are locked dependencies excluded by the
selected features or target. Both groups were audited. Keep the lockfile with
the source and use `--locked` for reproducible builds.

## Argument parser and approved exception

```toml
clap = { version = "4.6.7", default-features = false, features = ["std", "derive", "help", "usage", "error-context"] }
```

The `derive` feature supplies `clap_derive` through Clap's normal reexports;
it does not need a separate direct dependency. Help, usage, and error context
are enabled. Color, suggestions, wrapping, and Markdown support are disabled,
so their optional dependencies are absent. The selected Clap release requires
Rust 1.85.

| Added crate | Locked version | Release date | License | Upstream |
| --- | --- | --- | --- | --- |
| clap | 4.6.7 | 2026-09-14 | MIT OR Apache-2.0 | [clap-rs/clap](https://github.com/clap-rs/clap) |
| clap_builder | 4.6.7 | 2026-09-14 | MIT OR Apache-2.0 | [clap-rs/clap](https://github.com/clap-rs/clap) |
| clap_derive | 4.6.7 | 2026-09-14 | MIT OR Apache-2.0 | [clap-rs/clap](https://github.com/clap-rs/clap) |
| clap_lex | 1.1.1 | 2026-09-14 | MIT OR Apache-2.0 | [clap-rs/clap](https://github.com/clap-rs/clap) |
| anstyle | 1.0.14 | 2026-03-13 | MIT OR Apache-2.0 | [rust-cli/anstyle](https://github.com/rust-cli/anstyle) |
| heck | 0.5.0 | 2024-03-12 | MIT OR Apache-2.0 | [withoutboats/heck](https://github.com/withoutboats/heck) |

All six are present in the active normal/build graph. `clap_derive` also uses
the existing `proc-macro2`, `quote`, and `syn` versions listed below; no duplicate
versions are introduced. `clap_lex`, `anstyle`, and `heck` have no dependencies.
The Clap and anstyle upstreams are unarchived and maintained, and their
selected versions qualify through stable versions above 1.0 and recent
releases.

**Approved exception: `heck` 0.5.0.** It is an unavoidable dependency of the
requested `clap_derive` feature. Its version is below 1.0, its release was on
2024-03-12, and its latest upstream maintenance was
[tests](https://github.com/withoutboats/heck/commit/df22272ad96508108ec7fdf0a716bfbe82184337)
and [CI updates](https://github.com/withoutboats/heck/commit/252c1906cefefb8d13ecad7fce7b8f27ce6437e0)
on 2025-08-09, outside the required twelve-month window. This exception was
reported before adoption. On **2026-10-14**, the user explicitly approved it:
“Allow the exception; use clap_derive”. Its upstream is not archived, and the
RustSec check found no advisory for it. This approval applies to `heck` for
Clap derives, not to other dependency-policy exceptions.

## Configuration dependencies

Configuration parsing uses the following dependency declarations.

```toml
serde = { version = "1.0.229", features = ["derive"] }
toml = { version = "1.1.8", default-features = false, features = ["std", "serde", "parse"] }
```

The TOML release's full version is `1.1.8+spec-1.1.0`; dependency requirements
omit its build metadata. Disabling default features excludes serialization and
debug colors from the `toml` parser. The separate `toml_edit` dependency enables the writer and ordered
maps for explicit, comment-preserving saves. The declared minimum Rust version
of every locked package is at most 1.85.

| Crate | Verified version | Release date | License | Upstream |
| --- | --- | --- | --- | --- |
| serde | 1.0.229 | 2026-07-18 | MIT OR Apache-2.0 | [serde-rs/serde](https://github.com/serde-rs/serde) |
| serde_core | 1.0.229 | 2026-07-18 | MIT OR Apache-2.0 | [serde-rs/serde](https://github.com/serde-rs/serde) |
| serde_derive | 1.0.229 | 2026-07-18 | MIT OR Apache-2.0 | [serde-rs/serde](https://github.com/serde-rs/serde) |
| toml | 1.1.8+spec-1.1.0 | 2026-10-09 | MIT OR Apache-2.0 | [toml-rs/toml](https://github.com/toml-rs/toml) |
| toml_parser | 1.1.5+spec-1.1.0 | 2026-10-09 | MIT OR Apache-2.0 | [toml-rs/toml](https://github.com/toml-rs/toml) |
| toml_datetime | 1.1.2+spec-1.1.0 | 2026-10-08 | MIT OR Apache-2.0 | [toml-rs/toml](https://github.com/toml-rs/toml) |
| serde_spanned | 1.1.2 | 2026-10-08 | MIT OR Apache-2.0 | [toml-rs/toml](https://github.com/toml-rs/toml) |
| winnow | 1.0.4 | 2026-07-13 | MIT | [winnow-rs/winnow](https://github.com/winnow-rs/winnow) |
| proc-macro2 | 1.0.107 | 2026-07-19 | MIT OR Apache-2.0 | [dtolnay/proc-macro2](https://github.com/dtolnay/proc-macro2) |
| quote | 1.0.47 | 2026-07-19 | MIT OR Apache-2.0 | [dtolnay/quote](https://github.com/dtolnay/quote) |
| syn | 3.0.9 | 2026-10-12 | MIT OR Apache-2.0 | [dtolnay/syn](https://github.com/dtolnay/syn) |
| unicode-ident | 1.0.27 | 2026-10-12 | (MIT OR Apache-2.0) AND Unicode-3.0 | [dtolnay/unicode-ident](https://github.com/dtolnay/unicode-ident) |

These are the twelve active configuration and shared macro crates, verified with
`cargo tree --locked --edges normal,build` and
`cargo tree --locked --edges features`. All have stable versions at least 1.0,
recent releases, and unarchived, maintained upstreams. The TOML parser has no
serialization feature enabled; the writer and ordered-map crates below are
activated separately by `toml_edit`. No debug-color or asynchronous-runtime
feature is enabled.

The following four packages were originally locked but inactive; all four now
occur in the active normal/build graph through `toml_edit`:

| TOML editor support crate | Version | Release date | License | Upstream |
| --- | --- | --- | --- | --- |
| equivalent | 1.0.3 | 2026-10-13 | Apache-2.0 OR MIT | [indexmap-rs/equivalent](https://github.com/indexmap-rs/equivalent) |
| hashbrown | 0.17.1 | 2026-05-09 | MIT OR Apache-2.0 | [rust-lang/hashbrown](https://github.com/rust-lang/hashbrown) |
| indexmap | 2.14.2 | 2026-09-05 | Apache-2.0 OR MIT | [indexmap-rs/indexmap](https://github.com/indexmap-rs/indexmap) |
| toml_writer | 1.1.3+spec-1.1.0 | 2026-10-08 | MIT OR Apache-2.0 | [toml-rs/toml](https://github.com/toml-rs/toml) |

All four qualify. In particular, `hashbrown` is below 1.0 but has a release
within six months and maintained, unarchived upstream, including an
[iterator fix merged on 2026-09-24](https://github.com/rust-lang/hashbrown/commit/7109e3a6388b0a6a0e843befc8876864e1d44d2e).
The other three qualify through stable versions at least 1.0 and maintained
upstreams; [equivalent's release](https://github.com/indexmap-rs/equivalent/commit/199fd8460ab195708e40aac61b5df2962189b14d)
and [indexmap's release](https://github.com/indexmap-rs/indexmap/commit/0fb7b5c813c94dcb16f220d249e42a7b9e4ea44d)
provide additional dated evidence. No policy exception is needed for these
configuration dependencies. Terminal dependencies have their own exception
recorded below.

The twenty-two original package versions were checked against the live
crates.io version metadata: all were the current stable version,
non-yanked, and had a checksum matching `Cargo.lock`. All thirteen upstream
repositories were checked as unarchived; the maintenance-policy exception for
`heck` is explicit above. This is a metadata
and dependency-graph audit, not a claim to have reviewed every dependency's
source code.

The initially surprising recent version numbers were independently checked a
second time: [Serde's version API](https://crates.io/api/v1/crates/serde/1.0.229)
and [release-tag manifest](https://github.com/serde-rs/serde/blob/v1.0.229/serde/Cargo.toml)
agree on 1.0.229; [TOML's version API](https://crates.io/api/v1/crates/toml/1.1.8%2Bspec-1.1.0)
and [upstream release manifest](https://github.com/toml-rs/toml/blob/ceac6e4412420d5a5c19e4f7293ad005cf1a31b2/crates/toml/Cargo.toml)
agree on 1.1.8+spec-1.1.0. Both API records were non-yanked. The recheck's HTTP
response date was 2026-10-14.

## Inline terminal interaction and explicit configuration saving

```toml
crossterm = { version = "0.29.0", default-features = false, features = ["events"] }
toml_edit = "0.25.17"
```

Crossterm provides keyboard-event polling, raw-terminal handling, and cursor
commands without an alternate screen. Only `events` is enabled: no async event
stream, clipboard, mouse opt-in, Windows backend, derive helpers, or extra UI
framework is needed. Its internal locks and event handling run on the control
thread, never in an audio callback. TOML edit's default `parse` and `display`
features preserve comments and formatting when the user explicitly saves;
Serde integration and debug features are disabled. The full editor release is
`0.25.17+spec-1.1.0`, with build metadata omitted from the dependency requirement.

| Added active crate | Locked version | Release date | License | Upstream |
| --- | --- | --- | --- | --- |
| crossterm | 0.29.0 | 2025-04-05 | MIT | [crossterm-rs/crossterm](https://github.com/crossterm-rs/crossterm) |
| bitflags | 2.13.2 | 2026-09-10 | MIT OR Apache-2.0 | [bitflags/bitflags](https://github.com/bitflags/bitflags) |
| document-features | 0.2.12 | 2025-10-24 | MIT OR Apache-2.0 | [slint-ui/document-features](https://github.com/slint-ui/document-features) |
| litrs | 1.0.1 | 2026-10-12 | MIT OR Apache-2.0 | [LukasKalbertodt/litrs](https://github.com/LukasKalbertodt/litrs) |
| mio | 1.2.4 | 2026-10-03 | MIT | [tokio-rs/mio](https://github.com/tokio-rs/mio) |
| libc | 0.2.190 | 2026-10-02 | MIT OR Apache-2.0 | [rust-lang/libc](https://github.com/rust-lang/libc) |
| log | 0.4.34 | 2026-08-22 | MIT OR Apache-2.0 | [rust-lang/log](https://github.com/rust-lang/log) |
| parking_lot | 0.12.5 | 2025-10-03 | MIT OR Apache-2.0 | [Amanieu/parking_lot](https://github.com/Amanieu/parking_lot) |
| parking_lot_core | 0.9.12 | 2025-10-03 | MIT OR Apache-2.0 | [Amanieu/parking_lot](https://github.com/Amanieu/parking_lot) |
| lock_api | 0.4.14 | 2025-10-03 | MIT OR Apache-2.0 | [Amanieu/parking_lot](https://github.com/Amanieu/parking_lot) |
| scopeguard | 1.2.0 | 2023-07-17 | MIT OR Apache-2.0 | [bluss/scopeguard](https://github.com/bluss/scopeguard) |
| cfg-if | 1.0.5 | 2026-09-16 | MIT OR Apache-2.0 | [rust-lang/cfg-if](https://github.com/rust-lang/cfg-if); approved exception below |
| smallvec | 1.16.3 | 2026-10-10 | MIT OR Apache-2.0 | [servo/rust-smallvec](https://github.com/servo/rust-smallvec) |
| rustix | 1.1.5 | 2026-09-16 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | [bytecodealliance/rustix](https://github.com/bytecodealliance/rustix) |
| errno | 0.3.14 | 2025-09-09 | MIT OR Apache-2.0 | [lambda-fairy/rust-errno](https://github.com/lambda-fairy/rust-errno) |
| signal-hook | 0.3.18 | 2025-05-09 | Apache-2.0/MIT | [vorner/signal-hook](https://github.com/vorner/signal-hook) |
| signal-hook-registry | 1.4.8 | 2025-12-25 | MIT OR Apache-2.0 | [vorner/signal-hook](https://github.com/vorner/signal-hook) |
| signal-hook-mio | 0.2.5 | 2025-10-26 | MIT OR Apache-2.0 | [vorner/signal-hook](https://github.com/vorner/signal-hook) |
| toml_edit | 0.25.17+spec-1.1.0 | 2026-10-09 | MIT OR Apache-2.0 | [toml-rs/toml](https://github.com/toml-rs/toml) |

Every upstream in this table was checked as unarchived except the explicit
`cfg-if` exception. Stable versions at least 1.0 qualify where the project is
not abandoned; `scopeguard` has no recent release, but is a stable, unarchived
small utility with no abandonment notice or RustSec advisory. For releases
below 1.0 that are older than twelve months, substantive maintenance supplies
the additional evidence:

- Crossterm has an [input-parser fix from 2026-06-02](https://github.com/crossterm-rs/crossterm/commit/d4e9929f9a1c4adf91505d54f8527c817ae504cd)
  and [terminal-size maintenance from 2026-10-07](https://github.com/crossterm-rs/crossterm/commit/d903fb75619f29ab2d5a4947bbd3dae1bbc0153e).
  Version 0.29.0 is still the latest published stable release.
- The parking_lot workspace has [lock API work from 2026-10-03](https://github.com/Amanieu/parking_lot/commit/df8e469c83f9d6374097142270077763296bd07b)
  and a [core spin-count fix from 2026-10-07](https://github.com/Amanieu/parking_lot/commit/7d52c89a7cd04910e0a762b062bd778d519ea1b0).
- errno has [new platform support from 2026-08-06](https://github.com/lambda-fairy/rust-errno/commit/58dda7e9b3610fbeeb0b8127c5bc38ce926b9d00).
- signal-hook's shared upstream has [close-on-exec maintenance from 2026-10-04](https://github.com/vorner/signal-hook/commit/d12e7d9973397a85fe75ca0bc739d7ce01451a7d).
  Crossterm's constraints resolve signal-hook 0.3.18 and signal-hook-mio 0.2.5;
  their newer incompatible releases are 0.4.5 and 0.3.0 respectively. The
  selected versions are non-yanked and have no applicable RustSec advisory.
- document-features and signal-hook-mio have releases within twelve months;
  libc and log have releases within six months. TOML edit has both a current
  release and a [display fix from 2026-10-09](https://github.com/toml-rs/toml/commit/e3a64aa201ec339abacb570417d75b327490cab4).

**Approved exception: `cfg-if` 1.0.5.** Crossterm's mandatory
`parking_lot` → `parking_lot_core` chain requires it. The upstream is archived,
and its [2026-09-16 deprecation notice](https://github.com/rust-lang/cfg-if/commit/8b0a2b0fc1fa3b3741b9e72895f390cf499fe36f)
says the crate was replaced by Rust's `cfg_select!`, promising only breakage
and security fixes. Its stable 1.0 version therefore does not satisfy the
non-archived-upstream requirement. This was reported before adoption, and on
**2026-10-14** the user approved: “Allow the cfg-if exception and use Crossterm”.
No RustSec advisory was present for cfg-if. The existing `heck` exception
remains separate and unchanged.

The following six packages are locked but inactive in the macOS normal/build
graph. They were also checked for licenses, release metadata, and advisories.

| Inactive crate | Locked version | Release date | License | Upstream |
| --- | --- | --- | --- | --- |
| linux-raw-sys | 0.12.1 | 2025-12-23 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | [sunfishcode/linux-raw-sys](https://github.com/sunfishcode/linux-raw-sys) |
| memchr | 2.8.3 | 2026-07-08 | Unlicense OR MIT | [BurntSushi/memchr](https://github.com/BurntSushi/memchr) |
| redox_syscall | 0.5.18 | 2025-10-04 | MIT | [Redox upstream](https://gitlab.redox-os.org/redox-os/syscall), [maintained mirror](https://github.com/redox-os/syscall) |
| wasi | 0.11.1+wasi-snapshot-preview1 | 2025-06-10 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | [bytecodealliance/wasi](https://github.com/bytecodealliance/wasi) |
| windows-link | 0.2.1 | 2025-10-06 | MIT OR Apache-2.0 | [microsoft/windows-rs](https://github.com/microsoft/windows-rs) |
| windows-sys | 0.61.2 | 2025-10-06 | MIT OR Apache-2.0 | [microsoft/windows-rs](https://github.com/microsoft/windows-rs) |

These upstreams are unarchived and maintained. linux-raw-sys qualifies through
its release, memchr through its stable version and release, and the older
platform bindings through substantive shared-project maintenance:
[Redox memory-policy work in August 2026](https://github.com/redox-os/syscall/commit/4cc3baafdcbca65660d7ad4c52b7900e1fd32315),
[WASI work in September 2026](https://github.com/bytecodealliance/wasi/commit/43007ef51303552f60fd6f1528ec5f778c33e87e),
and [Windows bindings in October 2026](https://github.com/microsoft/windows-rs/commit/cc1bf12e71bc6c355de0a5090b6a8b529491e498).
The Redox GitLab API was unavailable; the unarchived upstream GitHub mirror
supplies that maintenance evidence. Cargo selects the compatible older lines
of redox_syscall, wasi, and windows-link rather than their newer incompatible
0.9.4, 0.14.7+wasi-0.2.4, and 0.100.0 releases.

All twenty-five newly locked versions were checked against live crates.io
metadata as non-yanked, with checksums matching the resolved lockfile. The
active graph has no duplicate package versions. Console 0.16.6 and Dialoguer
0.12.0 were considered: their key/prompt interfaces block, so they would need
additional input-thread or low-level terminal handling to keep the health
monitor responsive. Inquire 0.9.4 adds prompt abstractions and dependencies
without simplifying continuous arrow-key adjustment. Termion 4.0.6 was also
considered; its asynchronous reader has an unbounded queue and detached thread.
Crossterm supplies event parsing. Version 0.29.0 can block on incomplete escape
or UTF-8 input despite a zero-timeout poll, so ODAUD temporarily makes stdin
nonblocking with narrow macOS FFI and restores its flags on exit. A PTY
regression covers partial input and SIGTERM shutdown. No mouse reporting or
cursor-position queries are enabled.

## Audio and UI candidates evaluated, not adopted

| Candidate | Current release and date | License | Finding |
| --- | --- | --- | --- |
| [objc2](https://github.com/madsmtm/objc2) | 0.6.5, 2026-10-05 | MIT | Maintained Objective-C bindings; eligible if needed. |
| objc2-core-audio | 0.3.2, 2025-10-04 | Zlib OR Apache-2.0 OR MIT | CoreAudio aggregate and process-tap bindings; shared upstream has substantive 2026 maintenance. |
| objc2-core-audio-types | 0.3.2, 2025-10-04 | Zlib OR Apache-2.0 OR MIT | Shared upstream maintained; component-specific age needs clarification before adoption, described below. |
| objc2-core-foundation | 0.3.2, 2025-10-04 | Zlib OR Apache-2.0 OR MIT | Shared objc2 upstream maintained. |
| objc2-foundation | 0.3.2, 2025-10-04 | MIT | Shared objc2 upstream maintained. |
| objc2-app-kit | 0.3.2, 2025-10-04 | Zlib OR Apache-2.0 OR MIT | Native AppKit is the smallest prospective UI; GUI is deferred. |
| [cidre](https://github.com/yury/cidre) | 0.29.0, 2026-09-10 | MIT | Maintained alternative bindings; disable default `full` features, which include private APIs and unnecessary frameworks. |
| [coreaudio-rs](https://github.com/RustAudio/coreaudio-rs) | 0.14.2, 2026-04-29 | MIT OR Apache-2.0 | Maintained; now wraps objc2 bindings. Adds a layer unnecessary for the direct aggregate/tap experiment. |
| [coreaudio-sys](https://github.com/RustAudio/coreaudio-sys) | 0.2.18, 2026-06-04 | MIT | Maintained; build-time bindgen adds dependencies. |
| [CPAL](https://github.com/RustAudio/cpal) | 0.18.2, 2026-08-16 | Apache-2.0 | Maintained directly, but mandatory `dasp_sample` fails the stated dependency age/version policy. |
| [Rubato](https://github.com/HEnquist/rubato) | 5.0.1, 2026-10-01 | MIT OR Apache-2.0 | Maintained directly, but mandatory `windowfunctions` fails the stated policy. Prefer Core Audio's aggregate clock management when feasible. |
| [rtrb](https://github.com/mgeier/rtrb) | 0.4.0, 2026-08-17 | MIT OR Apache-2.0 | Eligible if a cross-thread audio queue becomes necessary; no normal dependencies. Includes the current RustSec fix. |
| [ringbuf](https://github.com/agerasev/ringbuf) | 0.5.3, 2026-10-08 | MIT OR Apache-2.0 | Eligible alternative; adds `crossbeam-utils`. Includes the current RustSec fix. |

Every main upstream in this table was verified non-archived. Concrete
maintenance and significant transitive findings:

- objc2's CoreAudio bindings were [updated for Xcode on 2026-09-24](https://github.com/madsmtm/objc2/commit/b735fb4d6b9c3be2a12a73a97a6d0fdc8d724a9b)
  and had a [type correction on 2026-06-21](https://github.com/madsmtm/objc2/commit/2ed4d2b332483be58bf0f20b3389cf35d6a423e7).
  Foundation and AppKit also have substantive 2026 changes. However,
  `objc2-core-audio-types`' last component-specific change/release was
  2025-10-04, just outside twelve months. Shared-project maintenance supports
  eligibility at the upstream-project level; a strict per-component reading
  requires reporting an exception before adoption. No exception is assumed.
- objc2's required `objc2-encode` 4.1.0 is MIT and qualifies through its stable
  version and maintained shared upstream. Narrow framework features avoid
  optional block/dispatch libraries. `bitflags` 2.13.2 (2026-09-10, MIT OR
  Apache-2.0) and `libc` 0.2.190 (2026-10-02, MIT OR Apache-2.0) have maintained,
  unarchived [bitflags](https://github.com/bitflags/bitflags) and
  [libc](https://github.com/rust-lang/libc) upstreams if selected features need
  them. No complete objc2 feature graph was resolved.
- cidre's required `cidre-macros` 0.12.0 (2026-09-09, MIT) has no dependencies;
  its build dependency `cc` 1.8.0 (2026-10-11, MIT OR Apache-2.0) brings
  `find-msvc-tools` 0.1.14 (2026-09-25) and `shlex` 2.0.1 (2026-05-17), both
  MIT OR Apache-2.0. This is candidate evidence, not a completed cidre feature
  or build-script audit.
- CPAL requires `dasp_sample` 0.11.0, released 2020-05-29. Its former
  `RustAudio/sample` repository redirects to [RustAudio/dasp](https://github.com/RustAudio/dasp),
  whose latest maintenance commit is [2025-09-09](https://github.com/RustAudio/dasp/commit/d0892971a3b72dee9b3ff0d16f94b0962605e858).
  Neither the version nor date qualifies on the audit date. An exception
  would be required before adopting this CPAL version.
- Rubato requires `windowfunctions` 0.1.1, released 2025-02-21; its
  [latest upstream commit](https://github.com/HEnquist/windowfunctions-rs/commit/9be08f67a0a13a6ede870595a670d45eb7844a64)
  has the same date. An exception would be required. Its `audioadapter` 5.0.0
  and `audioadapter-buffers` 5.2.0 have recent releases. The latter's metadata
  points to an archived repository, but its code moved into the active
  [audioadapter workspace](https://github.com/HEnquist/audioadapter-rs/blob/5ae168d2bc8cbdaf60e82bddffb71a555d6d81cd/Cargo.toml).
  The archived old location alone is not evidence of abandonment.
- rtrb has [substantive maintenance on 2026-09-13](https://github.com/mgeier/rtrb/commit/494aea450b4a686682cd0e941ff4317f967c8f23).
  ringbuf's required `crossbeam-utils` 0.8.23 (2026-09-05, MIT OR Apache-2.0)
  has a maintained, unarchived [crossbeam upstream](https://github.com/crossbeam-rs/crossbeam).

## Security check and future additions

The [RustSec database tree checked](https://github.com/RustSec/advisory-db/tree/6aed84668c23d7f535077a9fe918bf1fa0f6a5bc)
contained no advisory entries for the original eighteen active crates or
`equivalent`, `indexmap`, and `toml_writer`. The original graph's one matching
entry was
[RUSTSEC-2024-0402](https://rustsec.org/advisories/RUSTSEC-2024-0402.html),
non-canonical Borsh serialization in `hashbrown` 0.15.0. It is patched in
0.15.1 and does not affect locked version 0.17.1.

The same live database was rechecked for all newly locked terminal/editor
packages. Matching advisories apply only to older versions:

- [RUSTSEC-2020-0070](https://rustsec.org/advisories/RUSTSEC-2020-0070.html):
  lock_api guard thread-safety issues, patched in 0.4.2; selected 0.4.14.
- [RUSTSEC-2020-0081](https://rustsec.org/advisories/RUSTSEC-2020-0081.html) and
  [RUSTSEC-2024-0019](https://rustsec.org/advisories/RUSTSEC-2024-0019.html):
  mio socket layout and Windows named-pipe issues, patched in 0.7.6 and 0.8.11;
  selected 1.2.4.
- smallvec advisories RUSTSEC-2018-0003, RUSTSEC-2018-0018,
  RUSTSEC-2019-0009, RUSTSEC-2019-0012, and
  [RUSTSEC-2021-0003](https://rustsec.org/advisories/RUSTSEC-2021-0003.html):
  historical memory-safety issues, all fixed by 1.6.1; selected 1.16.3.

No known unpatched RustSec advisory applies to the forty-seven locked packages.

The same database check found no entries for the listed objc2 framework
crates, cidre, coreaudio-rs, CPAL, or Rubato. Relevant audio queue advisories
were present:

- [RUSTSEC-2026-0274](https://rustsec.org/advisories/RUSTSEC-2026-0274.html):
  rtrb panic-safety double-free/use-after-free; patched by 0.3.5 and 0.4.0.
- [RUSTSEC-2026-0293](https://rustsec.org/advisories/RUSTSEC-2026-0293.html):
  ringbuf panic-safety double-free/use-after-free; patched in 0.5.2.
- [RUSTSEC-2022-0041](https://rustsec.org/advisories/RUSTSEC-2022-0041.html):
  crossbeam-utils atomic alignment issue; patched in 0.8.7.

The candidate queue versions above include these fixes. Absence from the
advisory database is not a claim that every upstream issue was audited. Before
adding another dependency, recheck its actual resolved normal/build
dependencies, report policy exceptions, record the selected features and
versions, and update and commit `Cargo.lock`. Dependency validation does not
substitute for real macOS audio-flow validation.
