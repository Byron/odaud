# Dependency evidence

Checked on **2026-10-14** against the live crates.io API, published crate
manifests, upstream GitHub repositories, and the RustSec advisory database.

## Adopted dependencies

The audio engine uses **no third-party audio dependencies**. It uses Rust's
standard library and narrowly scoped Rust FFI to Apple's installed Core Audio,
Core Foundation, and IOKit frameworks. These are platform
facilities, not vendored implementations of audio primitives. No UI toolkit,
resampler, ring-buffer crate, plugin host, or asynchronous runtime is used.
Serde and TOML are adopted for configuration parsing, as detailed below.

The native framework definitions must be checked against the installed macOS
SDK. Rust owns callback state and buffers; Apple's frameworks retain their
platform implementation and licensing. Build tools such as Cargo, rustc, and
Apple's linker are build prerequisites, not application runtime dependencies.

`Cargo.lock` records sixteen third-party packages. Twelve occur in the active
normal/build graph; four are locked optional dependencies excluded by the
selected features. Both groups were audited. Keep the lockfile with the source
and use `--locked` for reproducible builds.

## Configuration dependencies

The configuration-first CLI adopts the following dependency declarations.
Argument handling uses `std::env`.

```toml
serde = { version = "1.0.229", features = ["derive"] }
toml = { version = "1.1.8", default-features = false, features = ["std", "serde", "parse"] }
```

The TOML release's full version is `1.1.8+spec-1.1.0`; dependency requirements
omit its build metadata. Disabling default features excludes the TOML writer,
ordered-map preservation, debug colors, and their dependencies from the active
build. Some optional packages remain in Cargo's lockfile. The declared minimum
Rust version of every locked package is at most 1.85.

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

These are the twelve active third-party crates, verified with
`cargo tree --locked --edges normal,build` and
`cargo tree --locked --edges features`. All have stable versions at least 1.0,
recent releases, and unarchived, maintained upstreams. The active feature tree
contains no serializer, ordered-map, debug-color, or additional runtime crate.

The following four packages also appear in `Cargo.lock`, but do not appear in
the active normal/build graph:

| Locked optional crate | Version | Release date | License | Upstream |
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
provide additional dated evidence. No policy exception is needed for the
resolved graph.

Every one of the sixteen locked package versions was rechecked against the
live crates.io version metadata: all were the current stable version,
non-yanked, and had a checksum matching `Cargo.lock`. All ten upstream
repositories were rechecked as unarchived and maintained. This is a metadata
and dependency-graph audit, not a claim to have reviewed every dependency's
source code.

The initially surprising recent version numbers were independently checked a
second time: [Serde's version API](https://crates.io/api/v1/crates/serde/1.0.229)
and [release-tag manifest](https://github.com/serde-rs/serde/blob/v1.0.229/serde/Cargo.toml)
agree on 1.0.229; [TOML's version API](https://crates.io/api/v1/crates/toml/1.1.8%2Bspec-1.1.0)
and [upstream release manifest](https://github.com/toml-rs/toml/blob/ceac6e4412420d5a5c19e4f7293ad005cf1a31b2/crates/toml/Cargo.toml)
agree on 1.1.8+spec-1.1.0. Both API records were non-yanked. The recheck's HTTP
response date was 2026-10-14.

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
contained no advisory entries for the twelve active configuration crates or
the locked `equivalent`, `indexmap`, and `toml_writer` packages. The one matching
entry for the sixteen locked packages was
[RUSTSEC-2024-0402](https://rustsec.org/advisories/RUSTSEC-2024-0402.html),
non-canonical Borsh serialization in `hashbrown` 0.15.0. It is patched in
0.15.1 and does not affect locked version 0.17.1. No known unpatched RustSec
advisory applies to the resolved graph.

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
