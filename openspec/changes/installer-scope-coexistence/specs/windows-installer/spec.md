## ADDED Requirements

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
