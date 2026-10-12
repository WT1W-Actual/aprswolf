# APRSWolf Releases

This folder contains versioned release notes for published APRSWolf builds.

## Available Releases

- [v1.0.4](1.0.4.md)
- [v1.0.3](1.0.3.md)
- [v1.0.2](1.0.2.md)
- [v1.0.1](1.0.1.md)
- [v1.0](1.0.md)
- [v1.0rc1](1.0rc1.md)
- [v0.9](0.9.md)
- [v0.34b](0.34b.md)
- [v0.34a](0.34a.md)
- [v0.33.0](0.33.0.md)
- [v0.32.0](0.32.0.md)

## How To Add A New Release

1. Copy [../RELEASE-NOTES-TEMPLATE.md](../RELEASE-NOTES-TEMPLATE.md)
2. Save it as `docs/releases/X.Y.Z.md`
3. Fill in the shipped changes, download names, and upgrade notes
4. Add the new version to the list above

## Multi-Platform Artifacts

Releases are published by the `release` GitHub Actions workflow when a `v*` tag
is pushed. Each release attaches artifacts for all supported platforms:

- macOS: Intel (`x64`) and Apple Silicon (`aarch64`) DMGs
- Linux: `.deb`, `.rpm`, and `.AppImage` for amd64 and arm64 (ARM64 Linux, e.g. Raspberry Pi OS 64-bit)
- Windows: MSI and NSIS `.exe` for x64

Note: the macOS `aarch64` DMG is for Apple Silicon Macs; the Linux `arm64`
packages are for ARM64 Linux. They are not interchangeable.

See the [project README](../../README.md#releases) for the full artifact table.
