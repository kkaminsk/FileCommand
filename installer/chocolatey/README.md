# Chocolatey package

This directory packages FileCommand as a [Chocolatey](https://chocolatey.org/)
package (`filecommand`). Like the [winget manifest](../winget/), it is a
**wrapper around the same `FileCommandSetup.exe`** produced by
[`../build.ps1`](../build.ps1) — the package downloads the bootstrapper at
install time, verifies its SHA-256, and runs it silently. No binaries are
embedded in the `.nupkg`.

## Files

- `template/filecommand.nuspec` — package metadata, with `<VERSION>` /
  `<REPO_URL>` placeholders (mirrors the winget template convention).
- `template/tools/chocolateyInstall.ps1` — downloads
  `<RELEASE_URL>/FileCommandSetup.exe`, enforces the stamped `<SHA256>`
  checksum via `Install-ChocolateyPackage`, and runs the bundle with
  `/quiet /norestart InstallScope=...`.
- `template/tools/chocolateyUninstall.ps1` — locates the Burn bundle's
  cached copy through its `Uninstall` registry registration (matching
  `FileCommandSetup.exe`, never a plain `MsiExec.exe` MSI entry) and
  re-runs it with `/uninstall /quiet /norestart`. The install script
  records the chosen scope in `install-scope.txt` so uninstall looks in
  the right hive on machines that carry both scopes at once; Burn also
  persists the original `InstallScope`, so the scope that was installed
  is the scope removed.
- `template/tools/LICENSE.txt`, `template/tools/VERIFICATION.txt` — the
  license and checksum-verification documentation the Chocolatey community
  repository requires for packages that download installers.
- `build.ps1` — stamps the placeholders, stages the package tree, and runs
  `choco pack`.

## Scope semantics

Chocolatey convention is a machine-wide install, so the package defaults to
`InstallScope=perMachine` (elevated — Chocolatey itself normally runs
elevated). A per-user install is available as a package parameter:

```powershell
choco install filecommand --params "'/Scope:user'"
```

This maps onto the same dual-scope semantics as the bootstrapper and winget
— see [../README.md](../README.md#scope-semantics). The same rule applies:
**same-scope upgrades only**; switching scope means uninstall-then-reinstall.

## Prerequisites

- `FileCommandSetup.exe` built first: `..\build.ps1` (pass `-Sign` for a
  release build — only ship packages pointing at **signed** installers).
- Chocolatey CLI for the pack step: https://chocolatey.org/install

## Building

```powershell
.\build.ps1
```

This reads the version and repository URL from the workspace `Cargo.toml`,
hashes `..\out\FileCommandSetup.exe`, and assumes the bootstrapper will be
published at `<repo>/releases/download/v<version>/FileCommandSetup.exe`
(the standard GitHub Releases layout). Override with `-ReleaseUrl`,
`-Version`, `-RepoUrl`, or `-SetupExe` as needed. Output:

- `..\out\chocolatey\pkg\` — the staged, fully substituted package tree
- `..\out\chocolatey\filecommand.<version>.nupkg` — the packed package

Pass `-StageOnly` to produce just the staged tree (handy on machines
without Chocolatey, or to review the substitutions).

## Testing

From an elevated shell, against the local `.nupkg`:

```powershell
choco install filecommand --source "'<repo>\installer\out\chocolatey;https://community.chocolatey.org/api/v2/'"
filecommand            # smoke test (new shell if PATH hasn't refreshed)
choco uninstall filecommand
```

## Publishing

Pushing to the Chocolatey community repository is a release-time step
performed by a maintainer, outside this repo (same as winget):

1. Build and sign the bootstrapper (`..\build.ps1 -Sign`) and publish it as
   a release asset at the URL baked into the package.
2. `.\build.ps1` in this directory.
3. `choco push filecommand.<version>.nupkg --source https://push.chocolatey.org/ --api-key <KEY>`

The package then goes through automated moderation (virus scan, validator,
verifier); `LICENSE.txt` and `VERIFICATION.txt` are already included for
that review.
