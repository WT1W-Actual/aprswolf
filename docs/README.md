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
- searching, clearing, exporting, and importing message history
- theme and sound settings
- remote web control
- troubleshooting

Use the in-app help reference for:

- the Help window topic map
- quick answers while the app is open
- a condensed user-oriented explanation of each major screen and workflow
- standard connection defaults and how to launch the remote UI

Screenshot assets used in the docs are stored under `docs/images/`.

### Release Management

Use the releases index and template for:

- versioned release notes
- multi-platform release notes
- upgrade notes
- known issues tracking

## Release Workflow

1. Update the package, Cargo, and Tauri versions together.
2. Copy [RELEASE-NOTES-TEMPLATE.md](RELEASE-NOTES-TEMPLATE.md) into `docs/releases/X.Y.Z.md` and describe user-visible changes.
3. Add the new release file to [releases/README.md](releases/README.md).
4. Check that the README download table, END-USER-GUIDE.md, and IN-APP-HELP.md describe the release.
5. Run `npm run verify` and either build on each target OS or push a `v*` tag to run the multi-platform release workflow.