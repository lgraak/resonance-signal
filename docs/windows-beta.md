# Windows Beta Packaging and Validation

## Supported beta platform

The downloadable beta package targets 64-bit Windows on the
`x86_64-pc-windows-msvc` Rust host. Linux/PipeWire packaging remains deferred.
The package is per-user, requires no installer or administrator access, and is
not a Windows Service.

The current application beta version is `0.1.0-beta.1`. This release version
is independent of consumer protocol version 1. Future beta builds increment
the prerelease suffix (`0.1.0-beta.2`, `0.1.0-beta.3`, and so on) until the
stable `0.1.0` release.

## Runtime model

Launching `resonance-agent.exe` from Explorer starts a user-session tray
process and the loopback consumer service on `127.0.0.1:48480`. The tray owns
the service lifecycle. Its menu reports the observed service state, shows the
endpoint, controls the owned Start with Windows value, and provides Exit.

The explicit `serve` and `capture` modes remain console diagnostics. `--tray`
is accepted as the explicit tray mode used by Windows startup registration.

Startup failures and lifecycle diagnostics are appended to:

```text
%LOCALAPPDATA%\Resonance Signal\logs\resonance-signal.log
```

The log is truncated when it reaches 1 MiB. It contains process/service
lifecycle and listener errors only. It does not contain audio samples, native
endpoint IDs, registry identity contents, credentials, or network telemetry.

## Start with Windows

The tray owns this per-user registry value:

```text
Key:   HKCU\Software\Microsoft\Windows\CurrentVersion\Run
Value: ResonanceSignal
Data:  "<absolute path to resonance-agent.exe>" --tray
```

The menu is checked only when the value data exactly names the current
executable, including the quoted path and explicit `--tray` argument (Windows
path casing is compared case-insensitively). Missing, unquoted, ambiguous, or
stale data is not reported as enabled. Selecting an unchecked stale entry is
an explicit request to replace the Resonance Signal-owned value with the
current command. Selecting a checked entry deletes only that owned value.

Moving the extracted folder makes an existing registration stale. Launch from
the new location and select Start with Windows to replace it, or remove the
owned value before moving the package.

## Release build and package

From the repository root on a 64-bit Windows MSVC Rust host:

```powershell
.\scripts\package-windows-beta.ps1
```

The script runs a locked optimized build and creates:

```text
dist\
  resonance-signal-0.1.0-beta.1-windows-x64\
    resonance-agent.exe
    LICENSE.txt
    README.txt
  resonance-signal-0.1.0-beta.1-windows-x64.zip
```

The script reads the `resonance-agent` version from Cargo metadata, builds the
release executable, and requires its `--version` output to match before naming
the directory and archive. `dist/` is generated release output and is excluded
from Git. Publish versioned ZIP assets through GitHub Releases; do not commit
them to the source repository.

The intended first-beta publication shape is documented but not yet created:

```text
Git tag:        v0.1.0-beta.1
GitHub Release: Resonance Signal v0.1.0-beta.1
Asset:          resonance-signal-0.1.0-beta.1-windows-x64.zip
```

The runtime has no repository-relative files and the package contains no
source tree, Cargo output other than the executable, private identity data, or
developer temporary files.

## Manual package acceptance

Validate the extracted package rather than the executable in `target/`:

1. Extract the ZIP to a new temporary directory outside the repository.
2. Run `resonance-agent.exe --version`, confirm `0.1.0-beta.1`, and retain
   that output with any tester report.
3. Launch `resonance-agent.exe` as a normal user.
4. Confirm the Resonance Signal notification-area icon and menu appear.
5. Confirm the tray reports `Status: Running` and `127.0.0.1:48480` is the
   only listener.
6. Request `/v1/status` and confirm `status: ready` and
   `listener_scope: loopback`.
7. Request `/v1/sources` and confirm a portable source snapshot is returned.
8. Connect a waveform consumer and confirm lifecycle plus `RSWF` frames.
9. Select Start with Windows; inspect the owned HKCU value and confirm it
   exactly names the packaged executable.
10. Select Start with Windows again; confirm the owned value is absent.
11. Select Exit; confirm the process terminates, the listener closes, and the
    consumer session ends.
12. Relaunch once and repeat the status and endpoint checks.

Do not reboot solely to validate registration. Leave Start with Windows
disabled unless the user explicitly wants it enabled after acceptance.

## Release validation checklist

- `cargo fmt --all --check`
- `cargo check --workspace --all-targets`
- `cargo test --workspace`
- `cargo doc --workspace --no-deps`
- `git diff --check`
- Cargo metadata, executable `--version`, and archive filename versions match
- release script completes on `x86_64-pc-windows-msvc`
- extracted package runs outside the repository
- tray status reflects both successful startup and bind failure
- Start with Windows enable, stale-state detection, and disable are verified
- `/v1/status`, `/v1/sources`, and `/v1/waveform` work from the package
- listener remains loopback-only and `0.0.0.0` remains rejected
- Exit closes active sessions and releases port 48480
- package contains only the executable, license, and beta README
- diagnostics contain no audio, private endpoint identity, or secrets

## Branding resources

The approved Resonance Signal artwork is retained under the repository-owned
branding directory:

```text
assets/branding/resonance-signal-icon.png
assets/branding/resonance-signal-icon.ico
assets/branding/resonance-signal-tray.ico
assets/branding/resonance-signal-banner.png
```

The Windows build embeds the application and tray ICO resources directly in
`resonance-agent.exe`. The release ZIP therefore remains limited to the
executable, license, and beta README; it has no runtime dependency on the
source-tree artwork.

## Current limitations

- Windows x64 only; Linux/PipeWire remains planned.
- No installer, updater, Windows Service, admin elevation, or automatic
  recovery.
- Local loopback clients only; no LAN binding, authentication, or TLS.
- Playback capture only; no microphone capture.
- Consumers are separate applications and are not packaged here.
