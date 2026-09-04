# Change: wix-installer

## Why

FileCommand ships only as a bare `cargo build` artifact — there is no installer, no PATH integration, and no way to distribute it through winget. A WiX-based installer makes the application deployable like real Windows software: a standard per-user install for unprivileged users, an administrator-driven per-machine install for shared boxes, `filecommand` on PATH either way, and a single bootstrapper executable that winget can drive silently.

## What Changes

- New **WiX (v4/v5) MSI package** with dual scope: **per-user by default, installing without elevation** to `%LOCALAPPDATA%\Programs\BigHatGroup\FileCommand`, or **per-machine with elevation** to `%ProgramFiles%\BigHatGroup\FileCommand` when explicitly selected. Both paths end in `BigHatGroup\FileCommand`.
- The install folder is **appended to PATH at the matching scope** — the user PATH for per-user installs, the machine PATH for per-machine installs — and removed again on uninstall, with the standard environment-change broadcast so new shells pick it up.
- New **Burn bootstrapper** `FileCommandSetup.exe` embedding the MSI compressed (WixStdBA UI), supporting full-UI, passive, and quiet modes, with a command-line control to select per-machine scope in silent installs.
- **Winget deployability**: stable UpgradeCode and ARP identity (Publisher `BigHatGroup`, name `FileCommand`, version from the Cargo workspace), Burn's standard silent switches, and a winget manifest template (`installerType: burn`) with per-scope installer switches checked into the repo. Actual submission to `winget-pkgs` is a release-time step outside this change.
- **Versioned upgrades**: `MajorUpgrade` keyed to the workspace version — installing a newer bundle upgrades in place at the same scope.
- A **reproducible build script** (`installer/build.ps1`) that builds the release binary and then the MSI and bundle; a Start Menu shortcut at the matching scope; an `installer/README.md` covering prerequisites (WiX toolset, .NET SDK) and the production signing step.

## Capabilities

### New Capabilities

- `windows-installer`: the WiX MSI + Burn bundle packaging of FileCommand — dual scope, install paths, PATH integration, upgrades, winget deployability, and the reproducible build.

### Modified Capabilities

*(none — no application behavior changes)*

## Impact

- **New directory `installer/`**: `Package.wxs` (MSI: directories, components, environment, shortcut, upgrade logic), `Bundle.wxs` (Burn bundle), `build.ps1`, `winget/` manifest template, `README.md`. **No changes to `crates/`** — the installer packages the existing `target/release/filecommand.exe`.
- **Toolchain:** requires WiX Toolset v4/v5 and the .NET SDK on the build machine; unsigned builds for development, code-signing documented as the production requirement.
- **Out of scope:** code-signing execution, winget-pkgs submission, auto-update, moving app configuration to `%APPDATA%`, non-Windows packaging.
