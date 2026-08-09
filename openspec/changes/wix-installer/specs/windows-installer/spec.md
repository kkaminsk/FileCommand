# windows-installer Specification (delta)

## ADDED Requirements

### Requirement: Dual-scope MSI package

The system SHALL ship a WiX-built MSI that installs FileCommand per-user by default **without any elevation prompt**, placing files in `%LOCALAPPDATA%\Programs\BigHatGroup\FileCommand`, and SHALL support an explicit per-machine mode that elevates and installs to `%ProgramFiles%\BigHatGroup\FileCommand`. Both install paths SHALL end in `BigHatGroup\FileCommand`. Each scope SHALL register standard Apps-list (ARP) metadata — Publisher `BigHatGroup`, product name `FileCommand`, the package version — and provide a Start Menu shortcut at the matching scope. Uninstalling SHALL remove the installed files and shortcut but SHALL NOT delete user-created runtime files.

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

The system SHALL ship a Burn bundle `FileCommandSetup.exe` that embeds the MSI compressed, so a single executable performs the entire install. The bundle SHALL present the WiX standard bootstrapper UI for interactive installs (per-user default), and SHALL support unattended operation via Burn's standard switches — quiet, passive, no-restart, and uninstall — with a documented command-line control selecting per-machine scope for silent installs.

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

The repository SHALL contain the complete installer source and a build script that produces the MSI and bundle from a clean checkout: verifying prerequisites (WiX toolset v4/v5, .NET SDK), building the release binary, stamping the workspace version, and compiling both packages. Documentation SHALL cover prerequisites, usage, scope semantics, the PATH-refresh caveat for already-open shells, and the production code-signing steps for the Burn engine and final bundle.

#### Scenario: Clean-checkout build

- **WHEN** the build script runs on a machine with the documented prerequisites
- **THEN** it produces the MSI and `FileCommandSetup.exe` with no manual steps

#### Scenario: Missing prerequisite fails clearly

- **WHEN** the build script runs without the WiX toolset installed
- **THEN** it stops with a message naming the missing prerequisite and how to install it
