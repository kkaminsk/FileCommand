# FileCommand installer

This directory contains the WiX (v4/v5) sources that package FileCommand
into a single bootstrapper executable, `FileCommandSetup.exe`, plus the
winget manifest template used to distribute it.

- `PackagePerUser.wxs` — the per-user MSI (`Scope="perUser"`): installs
  with **no elevation**, user PATH integration, Start Menu shortcut,
  versioned upgrades. This is the default scope.
- `PackagePerMachine.wxs` — the per-machine MSI (`Scope="perMachine"`):
  elevated install to Program Files, machine PATH, its own UpgradeCode
  (scope switches are uninstall-then-reinstall, never upgrades).
- `Bundle.wxs` — the Burn bundle that embeds **both** MSIs behind the WiX
  Standard Bootstrapper Application (WixStdBA) UI, producing
  `FileCommandSetup.exe`; exactly one MSI is planned per run, selected by
  the `InstallScope` variable. (Two single-scope MSIs replaced the
  original dual-scope design after testing showed WiX v5's Burn always
  elevates a dual-scope package — see design.md D1.)
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
  wix extension add WixToolset.BootstrapperApplications.wixext/5.0.2
  ```

  Pin the extension to `5.0.2` explicitly — `wix extension add` without a
  version resolves the latest release (currently 7.0.0), which is not
  compatible with the WiX v5 CLI and fails the bundle build with
  `WIX0144: The extension ... could not be found`.

`build.ps1` checks for `dotnet`, `wix`, and `cargo` on `PATH` up front and
fails fast with an install hint if any are missing.

## Building

From the repo root, or from this directory:

```powershell
.\installer\build.ps1
```

This runs `cargo build --release`, reads the current version from the
workspace `Cargo.toml` (`[workspace.package].version`), stamps it into
both MSIs' `ProductVersion` and the bundle's `Version`, and produces:

- `installer\out\PackagePerUser.msi`
- `installer\out\PackagePerMachine.msi`
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
per-machine one — is **not** an upgrade path: the two scopes are separate
MSIs with different UpgradeCodes, so installing at the other scope will
**coexist** with (not replace) the first install. To change scope:
uninstall the existing install, then install fresh at the new scope.

### PATH refresh in open shells

Windows Installer updates the registry PATH value and broadcasts
`WM_SETTINGCHANGE` on install/uninstall, which most well-behaved
applications (Explorer, new shell instances) pick up automatically.
**Shells already running at the time of install will not see the updated
`PATH`** — this is standard Windows behavior, not a bug in this
installer. Open a new terminal window to get `filecommand` on `PATH`
after installing.

## Production signing

Plain `build.ps1` produces **unsigned** development builds. Pass `-Sign`
to sign everything with Azure Trusted Signing (Big Hat Group Inc.'s
`BHGPublic/private-mcp` certificate profile, from the
[CodeSigning](https://github.com/kkaminsk/CodeSigning) tooling):

```powershell
.\installer\build.ps1 -Sign
```

This signs, in order: `filecommand.exe` (before packaging), both MSIs
(before the bundle build, so the bundle embeds already-signed MSIs), then
the Burn bundle itself via the two-pass sequence — `wix burn detach` to
extract the stub engine, sign it, `wix burn reattach`, then sign the
final `FileCommandSetup.exe`. (`wix burn detach`/`reattach` are the WiX
v4/v5 replacements for the old standalone `insignia` tool; see
https://docs.firegiant.com/wix/tools/signing/ for background.) Every
signed file is verified with `signtool verify /pa` immediately after
signing, and the whole build fails fast if any signature doesn't
validate.

`-Sign` requires, on the machine running the build:

- An authenticated `az login` / `Connect-AzAccount` session with the
  Code Signing Certificate Profile Signer role on the
  `BHGPublic/private-mcp` profile.
- A local checkout of the CodeSigning repo, for
  `Azure.CodeSigning.Dlib.dll` and the Trusted Signing metadata JSON.
  Defaults to `C:\GitHub\CodeSigning`; override with `-CodeSigningDlib`
  and `-CodeSigningMetadata` if it lives elsewhere.
- A Windows SDK signtool (10.0.26100.0+ recommended); auto-detected, or
  pass `-SignToolPath` explicitly.

Unsigned builds remain fine for local development and testing — `-Sign`
is opt-in, not the default, since most contributors won't have the
signing certificate's role assignment.

## Winget

See `winget/` for the manifest template and its own notes. Submitting to
`winget-pkgs` is a release-time step outside this repository.
