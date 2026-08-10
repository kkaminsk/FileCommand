# FileCommand installer

This directory contains the WiX (v4/v5) sources that package FileCommand
into a single bootstrapper executable, `FileCommandSetup.exe`, plus the
winget manifest template used to distribute it.

- `Package.wxs` — the MSI: dual-scope install (per-user by default,
  per-machine on request), PATH integration, Start Menu shortcut,
  versioned upgrades.
- `Bundle.wxs` — the Burn bundle that embeds the MSI behind the WiX
  Standard Bootstrapper Application (WixStdBA) UI, producing
  `FileCommandSetup.exe`.
- `License.rtf` — license text shown by the bootstrapper UI (FileCommand
  is dual-licensed MIT / Apache-2.0; see `LICENSE-MIT` / `LICENSE-APACHE`
  at the repo root).
- `build.ps1` — builds the release binary and both WiX artifacts.
- `winget/` — the winget package manifest template.

See `openspec/changes/wix-installer/design.md` for the rationale behind
these choices (dual-scope packaging, install paths, PATH handling, the
Burn scope switch, and versioning).

## Prerequisites

- **Rust toolchain** (`cargo`) — https://rustup.rs
- **.NET SDK** — https://dotnet.microsoft.com/download (required to run
  the WiX CLI, which is a .NET global tool)
- **WiX Toolset v4/v5 CLI**:

  ```powershell
  dotnet tool install --global wix
  wix extension add WixToolset.BootstrapperApplications.wixext
  ```

`build.ps1` checks for `dotnet`, `wix`, and `cargo` on `PATH` up front and
fails fast with an install hint if any are missing.

## Building

From the repo root, or from this directory:

```powershell
.\installer\build.ps1
```

This runs `cargo build --release`, reads the current version from the
workspace `Cargo.toml` (`[workspace.package].version`), stamps it into
both the MSI's `ProductVersion` and the bundle's `Version`, and produces:

- `installer\out\Package.msi`
- `installer\out\FileCommandSetup.exe`

Pass `-SkipCargoBuild` to repackage the existing
`target\release\filecommand.exe` without rebuilding Rust — useful while
iterating on the WiX authoring itself.

These are **unsigned development builds**. Unsigned executables will trip
SmartScreen on machines that haven't seen them before; see
[Production signing](#production-signing) below.

## Scope semantics

FileCommand installs to `BigHatGroup\FileCommand` under one of two roots,
selected by install scope:

| Scope | Path | Elevation | PATH scope |
|---|---|---|---|
| Per-user (default) | `%LocalAppData%\Programs\BigHatGroup\FileCommand` | None | User `PATH` (`HKCU`) |
| Per-machine | `%ProgramFiles%\BigHatGroup\FileCommand` | Elevates once (UAC) | Machine `PATH` (`HKLM`) |

Per-user is the default for both the interactive bootstrapper UI and
silent installs — the first-run experience never prompts for credentials
unless per-machine is explicitly requested.

### Selecting scope

- **Interactive**: `FileCommandSetup.exe` — installs per-user by default;
  the WixStdBA UI does not currently expose a scope picker
  (`SuppressOptionsUI="yes"`). To get a per-machine interactive install,
  launch elevated with the scope override below.
- **Silent, per-user** (default): `FileCommandSetup.exe /quiet`
- **Silent, per-machine**: `FileCommandSetup.exe /quiet InstallScope=perMachine`
  (must be run elevated; this is the switch winget's `--scope machine`
  maps to — see `winget/`)
- **Silent uninstall**: `FileCommandSetup.exe /uninstall /quiet`

Burn's standard switches (`/quiet`, `/passive`, `/norestart`, `/uninstall`,
`/log <path>`) all work as usual; only the `InstallScope` bundle variable
is FileCommand-specific.

### Same-scope upgrades only

Installing a newer bundle upgrades in place **at the same scope** the
existing install used (matched by `UpgradeCode`; see design.md decision
D5). Switching scope — e.g. going from a per-user install to a
per-machine one — is **not** an upgrade path. To change scope: uninstall
the existing install, then install fresh at the new scope.

### PATH refresh in open shells

Windows Installer updates the registry PATH value and broadcasts
`WM_SETTINGCHANGE` on install/uninstall, which most well-behaved
applications (Explorer, new shell instances) pick up automatically.
**Shells already running at the time of install will not see the updated
`PATH`** — this is standard Windows behavior, not a bug in this
installer. Open a new terminal window to get `filecommand` on `PATH`
after installing.

## Production signing

Development builds produced by `build.ps1` are unsigned. Before shipping
a release:

1. **Sign the MSI** (`installer\out\Package.msi`) with `signtool sign` (or
   your organization's signing pipeline) using an Authenticode
   certificate, *before* building the bundle — the bundle embeds the MSI
   as-is.
2. **Rebuild the bundle** (`wix build Bundle.wxs ...`) so it embeds the
   now-signed MSI.
3. **Sign the Burn engine and the final bundle.** Burn bundles require
   two signing passes: the stub engine is signed once WiX extracts it,
   then the final `FileCommandSetup.exe` is signed again. See the WiX
   documentation on signing Burn bundles
   (https://docs.firegiant.com/wix/tools/signing/) for the
   `insignia`-based re-signing sequence.
4. Verify with `signtool verify /pa` on both `Package.msi` and
   `FileCommandSetup.exe` before publishing.

Unsigned builds are accepted for local development and testing; signing
is a release-time gate, not something `build.ps1` performs automatically.

## Winget

See `winget/` for the manifest template and its own notes. Submitting to
`winget-pkgs` is a release-time step outside this repository.
