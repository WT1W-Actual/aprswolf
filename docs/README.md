# APRSWolf Documentation

This folder contains the main user-facing and release documentation for APRSWolf.

## Start Here

- End-user guide: [END-USER-GUIDE.md](END-USER-GUIDE.md)
- In-app help content: [IN-APP-HELP.md](IN-APP-HELP.md)
- Developer guide (architecture, code patterns, audit history): [DEVELOPER-GUIDE.md](DEVELOPER-GUIDE.md)
- Releases index: [releases/README.md](releases/README.md)
- Release notes template: [RELEASE-NOTES-TEMPLATE.md](RELEASE-NOTES-TEMPLATE.md)

## Documentation Map

### End Users

Use the end-user guide for:

- installation
- first-time setup
- connecting with TNC or APRS-IS
- sending and receiving messages
- theme and sound settings
- remote web control
- troubleshooting

Use the in-app help reference for:

- the Help window topic map
- quick answers while the app is open
- a condensed user-oriented explanation of each major screen and workflow

Screenshot assets used in the docs are stored under `docs/images/`.

### Release Management

Use the releases index and template for:

- versioned release notes
- DMG publish notes
- upgrade notes
- known issues tracking

## Typical Workflow

1. Read [END-USER-GUIDE.md](END-USER-GUIDE.md) for product usage documentation.
2. Build release artifacts with the macOS release commands from the project README.
3. Copy [RELEASE-NOTES-TEMPLATE.md](RELEASE-NOTES-TEMPLATE.md) into `docs/releases/X.Y.Z.md`.
4. Add the new release file to [releases/README.md](releases/README.md).
