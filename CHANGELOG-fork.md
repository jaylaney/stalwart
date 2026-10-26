# Fork changelog

This file records releases of the independent zero-access fork. Versioning, provenance,
compatibility, and release qualification follow [the release policy](docs/releases.md).
The inherited [CHANGELOG.md](CHANGELOG.md) records upstream Stalwart history.

## Unreleased

### Added

- Independent release policy: start at `0.1.0`, use fork release tags, and record the upstream
  base separately from the fork version.
- A separate fork changelog and links to the release policy from the project guides.

### Release preparation

- Current documented upstream base: Stalwart `0.16.26`. Record its exact commit, the fork
  release commit, and any additional backports when preparing a release.
- Package/runtime version changes and release workflow adaptation remain pending; this
  entry records documentation changes and does not announce a release.
