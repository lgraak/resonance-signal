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

Launching the Windows GUI-subsystem `resonance-agent.exe` from Explorer starts
a user-session tray process and the loopback consumer service on
`127.0.0.1:48480` without allocating a console. The tray owns the service
lifecycle. Its menu reports the observed service state, shows the endpoint,
controls Info/Debug logging and the diagnostics folder, controls the owned
Start with Windows value, and provides Exit.

The Windows console-subsystem `resonance-agent-cli.exe` retains `--help`,
`--version`, `serve`, and `capture` diagnostics. Separating the PE subsystem
entry points preserves synchronous terminal output while keeping normal tray
launch console-free. `--tray` remains the explicit tray mode accepted by
`resonance-agent.exe` for Windows startup registration.

Startup failures and lifecycle diagnostics are appended to:

```text
%LOCALAPPDATA%\Resonance Signal\logs\resonance-signal.log
```

The current log rotates before exceeding 1 MiB and retains two bounded history
files, for at most approximately 3 MiB of log content:

```text
resonance-signal.log
resonance-signal.1.log
resonance-signal.2.log
```

Info is the first-run and malformed-setting fallback. Debug is selected from
the tray and persists in `%LOCALAPPDATA%\Resonance Signal\settings.json`.
Lifecycle, listener, discovery, capture/session, panic, shutdown, and bounded
technical evidence are recorded according to that level. Logs do not contain
audio samples, native endpoint IDs, registry identity contents, credentials,
secrets, or per-frame waveform activity. Rust panics are recorded when the
panic hook runs; native process termination that bypasses Rust panic handling
cannot be captured by this architecture.

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
    resonance-agent-cli.exe
    LICENSE.txt
    README.txt
  resonance-signal-0.1.0-beta.1-windows-x64.zip
```

The script reads the `resonance-agent` version from Cargo metadata, builds both
release executables, requires `resonance-agent-cli.exe --version` to match, and
verifies the GUI and console PE subsystem values before naming the directory
and archive. `dist/` is generated release output and is excluded from Git.
Publish versioned ZIP assets through GitHub Releases; do not commit them to the
source repository.

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
2. Run `resonance-agent-cli.exe --version`, confirm `0.1.0-beta.1`, and retain
   that output with any tester report.
3. Launch `resonance-agent.exe` as a normal user.
4. Confirm the Resonance Signal notification-area icon and menu appear.
5. Confirm the tray reports `Status: Running` and `127.0.0.1:48480` is the
   only listener.
6. Request `/v1/status` and confirm `status: ready` and
   `listener_scope: loopback`.
7. Request `/v1/sources` and confirm a portable source snapshot is returned.
8. Connect a waveform consumer and confirm lifecycle plus `RSWF` frames.
9. Select **Open Diagnostics Folder** and confirm Explorer opens the current
   log directory without PowerShell, cmd, or Windows Terminal.
10. Select Debug, restart, and confirm the preference plus additional bounded
    lifecycle evidence persist; return to Info for the final default state.
11. Select Start with Windows; inspect the owned HKCU value and confirm it
   exactly names the packaged executable.
12. Select Start with Windows again; confirm the owned value is absent.
13. Select Exit; confirm the process terminates, the listener closes, and the
    consumer session ends.
14. Relaunch once and repeat the status and endpoint checks.

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
- package contains only the tray executable, console executable, license, and
  beta README
- tray executable is GUI-subsystem and console executable is console-subsystem
- Info/Debug preference, bounded rotation, panic formatting, and orderly exit
  evidence are verified
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
`resonance-agent.exe`. The release ZIP therefore remains limited to the tray
and CLI executables, license, and beta README; it has no runtime dependency on
the source-tree artwork.

## Current limitations

- Windows x64 only; Linux/PipeWire remains planned.
- No installer, updater, Windows Service, admin elevation, or automatic
  recovery.
- Local loopback clients only; no LAN binding, authentication, or TLS.
- Playback capture only; no microphone capture.
- Consumers are separate applications and are not packaged here.
