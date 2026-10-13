# windows-installer Specification

## Purpose
TBD - created by archiving change wix-installer. Update Purpose after archive.
## Requirements
### Requirement: Dual-scope MSI package

The system SHALL ship WiX-built MSI packaging that installs FileCommand per-user by default **without any elevation prompt**, placing files in `%LOCALAPPDATA%\Programs\BigHatGroup\FileCommand`, and SHALL support an explicit per-machine mode that elevates and installs to `%ProgramFiles%\BigHatGroup\FileCommand`. The two scopes MAY be delivered as two single-scope MSI packages behind one bundle — the no-elevation guarantee for the default scope takes precedence over single-package economy. Both install paths SHALL end in `BigHatGroup\FileCommand`. Each scope SHALL register standard Apps-list (ARP) metadata — Publisher `BigHatGroup`, product name `FileCommand`, the package version — and provide a Start Menu shortcut at the matching scope. Uninstalling SHALL remove the installed files and shortcut but SHALL NOT delete user-created runtime files.

#### Scenario: Per-user install needs no elevation

- **WHEN** a standard user runs the installer and accepts the default scope
- **THEN** no elevation prompt appears and `filecommand.exe` is installed under `%LOCALAPPDATA%\Programs\BigHatGroup\FileCommand`

#### Scenario: Per-machine install elevates and lands in Program Files

- **WHEN** the user selects per-machine scope
- **THEN** a single elevation prompt appears and `filecommand.exe` is installed under `%ProgramFiles%\BigHatGroup\FileCommand`

#### Scenario: Uninstall leaves user data alone

- **WHEN** an install of either scope is uninstalled
- **THEN** the installed files, shortcut, and ARP entry are removed
- **AND** any `config.toml`, `usermenu.toml`, or `history.json` the user's sessions created are untouched

### Requirement: Install directory joins PATH at the matching scope

Installing SHALL append the install directory to the PATH environment variable at the scope of the install — the **user** PATH for a per-user install, the **machine** PATH for a per-machine install — using Windows Installer environment authoring with the standard environment-change broadcast. Uninstalling SHALL remove exactly the entry it added. Newly started shells SHALL resolve `filecommand` after install.

#### Scenario: Per-user install extends the user PATH

- **WHEN** a per-user install completes and the user opens a new terminal
- **THEN** the user PATH contains `%LOCALAPPDATA%\Programs\BigHatGroup\FileCommand` and `filecommand` starts from any directory

#### Scenario: Per-machine install extends the machine PATH

- **WHEN** a per-machine install completes and any user opens a new terminal
- **THEN** the machine PATH contains `%ProgramFiles%\BigHatGroup\FileCommand` and `filecommand` resolves for that user

#### Scenario: Uninstall removes only its own PATH entry

- **WHEN** the product is uninstalled
- **THEN** the PATH entry the installer added is gone and all other PATH entries are byte-identical

### Requirement: Burn bootstrapper

The system SHALL ship a Burn bundle `FileCommandSetup.exe` that embeds the MSI package(s) compressed, so a single executable performs the entire install; when the scopes are separate MSIs, exactly one SHALL be planned for install per run, selected by the scope control. The bundle SHALL present the WiX standard bootstrapper UI for interactive installs (per-user default), and SHALL support unattended operation via Burn's standard switches — quiet, passive, no-restart, and uninstall — with a documented command-line control selecting per-machine scope for silent installs.

#### Scenario: One self-contained executable

- **WHEN** `FileCommandSetup.exe` is copied alone to a clean machine and run
- **THEN** the install completes without downloading or requiring any adjacent files

#### Scenario: Silent per-user install

- **WHEN** `FileCommandSetup.exe` runs with the quiet switch and no scope control
- **THEN** a per-user install completes with no UI and no elevation prompt

#### Scenario: Silent per-machine install

- **WHEN** `FileCommandSetup.exe` runs quiet with the documented per-machine scope control from an elevated context
- **THEN** a per-machine install completes with no UI

#### Scenario: Silent uninstall

- **WHEN** the bundle is invoked with its uninstall and quiet switches
- **THEN** the product is removed with no UI

### Requirement: Winget deployability

The bundle SHALL be deployable through winget: a stable bundle UpgradeCode and package identity (`BigHatGroup.FileCommand`), silent behavior compatible with winget's unattended flow, and a winget manifest template committed to the repository declaring `installerType: burn` with per-scope installer switches matching the bundle's documented controls. Submitting the manifest to the winget community repository SHALL remain a release-time step outside the build.

#### Scenario: Manifest template validates

- **WHEN** the committed manifest template is pointed at a built bundle and checked with winget's validation
- **THEN** validation passes and a local manifest-based install succeeds silently

#### Scenario: Identity is stable across versions

- **WHEN** two successive versions of the bundle are built
- **THEN** both carry the same UpgradeCode and package identifier, differing only in version

### Requirement: Versioned in-place upgrades

The MSI and bundle versions SHALL derive from the Cargo workspace version, and installing a newer bundle over an existing same-scope install SHALL upgrade in place, leaving exactly one Apps-list entry. Scope changes SHALL NOT be silent upgrades: moving between per-user and per-machine requires uninstall and reinstall, and the documentation SHALL say so.

#### Scenario: Same-scope upgrade

- **WHEN** version N is installed per-user and the version N+1 bundle is run per-user
- **THEN** the install upgrades in place and the Apps list shows a single FileCommand entry at version N+1

#### Scenario: Version flows from the workspace

- **WHEN** the workspace `Cargo.toml` version changes and the installer is rebuilt
- **THEN** the MSI ProductVersion, bundle version, and ARP display version all show the new value

### Requirement: Reproducible installer build

The repository SHALL contain the complete installer source and a build script that produces the MSI and bundle from a clean checkout: verifying prerequisites (WiX toolset v4/v5, .NET SDK), building the release binary, stamping the workspace version, and compiling every MSI package and the bundle. Documentation SHALL cover prerequisites, usage, scope semantics, the PATH-refresh caveat for already-open shells, and the production code-signing steps for the Burn engine and final bundle.

#### Scenario: Clean-checkout build

- **WHEN** the build script runs on a machine with the documented prerequisites
- **THEN** it produces the MSI and `FileCommandSetup.exe` with no manual steps

#### Scenario: Missing prerequisite fails clearly

- **WHEN** the build script runs without the WiX toolset installed
- **THEN** it stops with a message naming the missing prerequisite and how to install it

### Requirement: Bundle warns when the other installation scope is present

Before planning an install, the Burn bundle SHALL detect whether the MSI of the other scope is already installed, identifying it by that package's stable UpgradeCode:
- a run that will install per-user SHALL look for the per-machine package;
- a run that will install per-machine SHALL look for the per-user package of the account running setup.

When the other scope is present on an interactive run, the bundle's install page SHALL show a warning before the user starts the install. The warning SHALL name the scope that is already installed, SHALL state that both copies will be on PATH, and SHALL state which copy runs when the user types `filecommand`, which is always the per-machine copy, because the machine PATH precedes the user PATH. The install SHALL remain allowed, with no additional prompt and no change to the Install action. Quiet and passive runs SHALL NOT be blocked or prompted because of the other scope, and the detection result SHALL appear in the bundle's log.

#### Scenario: Per-user setup over an existing per-machine install

- **WHEN** FileCommand is installed per-machine and a user runs `FileCommandSetup.exe` interactively with the default per-user scope
- **THEN** the install page warns that FileCommand is already installed for all users, and that the all-users copy will start when `filecommand` is typed
- **AND** clicking Install completes a per-user install as it does today

#### Scenario: Per-machine setup over an existing per-user install

- **WHEN** FileCommand is installed per-user for the current account and the same account runs `FileCommandSetup.exe InstallScope=perMachine` interactively from an elevated context
- **THEN** the install page warns that FileCommand is already installed for this account, and that the new all-users copy will start when `filecommand` is typed
- **AND** the per-machine install proceeds when the user clicks Install

#### Scenario: No warning without the other scope

- **WHEN** neither scope is installed, or only the scope being installed or upgraded is present
- **THEN** the install page shows no scope warning

#### Scenario: Silent installs are unaffected

- **WHEN** `FileCommandSetup.exe /quiet` runs on a machine that already has the per-machine install
- **THEN** the per-user install completes with no UI and no prompt, exactly as before
- **AND** the bundle log records that the per-machine package was detected

### Requirement: Settings removal is documented

The installer documentation SHALL describe what uninstall leaves behind and how to remove it. It SHALL state that uninstalling either scope keeps the user's settings files (`config.toml`, `history.json`, `usermenu.toml`). It SHALL name their default location (`%APPDATA%\FileCommand`) and note that every account that ran FileCommand has its own copy. It SHALL give commands that delete that directory. It SHALL also describe the two non-default locations:
- portable mode, where the files and the `portable` marker live beside the executable in the install directory and remain after uninstall;
- `--config-dir`, where the files live in a user-chosen directory the installer never touches.

The installer itself SHALL NOT delete settings.

#### Scenario: A user looks up how to remove everything

- **WHEN** a user reads the uninstall section of `installer/README.md`
- **THEN** it states that uninstall keeps the settings, names `%APPDATA%\FileCommand`, and gives a PowerShell and a cmd command that remove it
- **AND** it explains the per-account copies of a per-machine install, and the portable-mode and `--config-dir` locations

#### Scenario: Uninstall still keeps settings

- **WHEN** either scope is uninstalled after the user has run FileCommand
- **THEN** `%APPDATA%\FileCommand` and its files are still present, and a later reinstall uses them

