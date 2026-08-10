# Winget manifest template

Template for the three-file winget manifest (`version`, `installer`,
`defaultLocale`) that publishes FileCommand as `BigHatGroup.FileCommand`
via `installerType: burn`. These files are **not** submitted anywhere
automatically — actual submission to
[`microsoft/winget-pkgs`](https://github.com/microsoft/winget-pkgs) is a
release-time step performed by a maintainer, outside this repo.

## Files

- `BigHatGroup.FileCommand.yaml` — version manifest.
- `BigHatGroup.FileCommand.installer.yaml` — installer manifest. Two
  `Scope` entries (`user`, `machine`) both point at the same
  `FileCommandSetup.exe`, differing only in `InstallerSwitches.Custom`,
  which forwards winget's scope choice to the bundle's `InstallScope`
  variable (see `../README.md`).
- `BigHatGroup.FileCommand.locale.en-US.yaml` — default locale metadata
  (publisher, description, tags, license).

## Filling in a release

For each release, after running `../build.ps1` and publishing
`FileCommandSetup.exe` (e.g. as a GitHub Release asset):

1. Replace `<VERSION>` in all three files with the released version (must
   match `PackageVersion` and the version stamped by `build.ps1`).
2. Replace `<RELEASE_URL>` in the installer manifest with the base URL
   the release asset is hosted at.
3. Compute the SHA-256 of the published `FileCommandSetup.exe` and
   replace `<SHA256>` in the installer manifest:

   ```powershell
   Get-FileHash .\FileCommandSetup.exe -Algorithm SHA256
   ```

4. Validate locally before submitting:

   ```powershell
   winget validate --manifest .\installer\winget\
   winget install --manifest .\installer\winget\
   ```

5. Submit the three files as a new version folder in `winget-pkgs`
   (`manifests/b/BigHatGroup/FileCommand/<VERSION>/`), per that
   repository's contribution guide, or use `wingetcreate` /
   `winget-pkgs`' automated PR workflow.

Only ship a manifest pointing at a **signed** `FileCommandSetup.exe` (see
`../README.md#production-signing`); unsigned bundles are for local
development only.
