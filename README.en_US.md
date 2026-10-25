<div align="center">

<img src="src-tauri/icons/icon.png" width="256" height="256" alt="Schematic Manager icon">

# Schematic Manager

**A cross-platform manager and previewer for Minecraft schematic (`.litematic`) files.**

English | [简体中文](README.md)

</div>

<img width="1560" height="980" alt="2026-10-12_010941" src="https://github.com/user-attachments/assets/d7855b1d-78b4-4957-9537-6c146bfee77f" />

> This program is distributed for free only; selling it in any form is prohibited.
> Author: 黑山大叔 — https://space.bilibili.com/19428259

## Features

| Feature | Description |
|---|---|
| Schematic list | After you pick a directory it recursively scans it for `.litematic` files and shows a summary of each one's size, block count and the version it was made with |
| 3D preview | Renders the schematic with real block models and textures, supports switching viewpoints and exporting a transparent-background PNG; files with more than 30,000 blocks are not rendered by default — press 「开始渲染」 (Start rendering) in the preview area (you have to confirm again after leaving that file) |
| Block counts | Counts by **the items needed to build it**, using the same accounting as the Litematica mod's material list: block states are first folded into items (torches and wall torches count as one, water and lava count only the single source bucket, doors and beds count once, double slabs count as two, snow layers / turtle eggs / sea pickles / candles use the count stored in the state, glow lichen counts by faces, a flower pot with a flower counts as pot plus plant), then summed per item; the「区分属性」(Split states) switch in the table header lists every block state on its own row — counts then become the number of blocks in that state and the state column is shown — while turning it off folds rows back by item and hides the state column; a row that spans several block shapes says so after its block name, with the exact names in the tooltip; a fixed action column on the far right carries one「替换」(Replace) entry per row |
| Block replacement | Replaces one block with another by rule, reached from the「替换」(Replace) entry at the right of each row in the block counts table; the window has two columns — the left holds the rules (the source block and its property constraints come from the row you clicked and cannot be edited, while the target block and target property overrides are yours to fill), the right lists every block in the library by block ID and can be narrowed with a keyword search, a click fills the target block; both panes show the display name in the current UI language, and the target field is outlined in red while empty; every replacement is backed up first, and the result can be restored from the backup |
| Version conversion | Converts a schematic to another game version: it analyses the differences first, then you assign substitutes for the missing blocks; the window has two columns — the left lists the gaps (red border = still to assign, green border = automatically recognised suggestion, no highlight = you picked it yourself), the right is a searchable block list you can click to fill in (blocks absent from the target version are marked and cannot be picked); the result is saved as a new file |
| Duplicate detection | Screens the loaded directory in place (subdirectories included): files that are byte-for-byte identical are listed as **duplicates**, moved to the top of the list, colour-coded per group, and the extra copies are checked by default; files with the same name, size and block count but different content are listed as **suspected duplicates** and only flagged, never checked |
| Backups and logs | Lists the backup taken before each modification, each of which can be restored or deleted; also carries a run log tab (with an entry to the 「日志管理」 (Log Management) window) |
| Log management | Shows the run log (block replacement, version conversion, rename, copy, delete and backup restore records); you can switch the level (error/warning/info/debug), refresh it, clear it, or open the log directory in the file manager; the log file rolls over once it exceeds 5 MB or every time the manager is reopened, archiving older content as `YYYY-MM-dd-<roll>.log.gz` (the number is a global rolling sequence: the highest existing archive plus one); logs live in `logs/` inside the application data directory (on Windows that is `%APPDATA%\io.github.heiheids.schematic-manager\logs`); reached from the run log tab of Backups and Logs |
| Metadata editing | Change a schematic's name, author and description |
| Updates | Shows the current version, the author and the author's homepage (clickable); 「检测更新」 (Check for updates) reads the release list on GitHub and decides whether a newer version exists (pre-releases included), and reports the result truthfully as one of "update available / already up to date / no releases found / check failed"; the download URL can be copied with a click, or handed to the browser via 「打开下载地址」 (Open download URL) |
| RMC schematic site | A placeholder entry at the top for an add-on program to plug into; clicking it does nothing until that is wired up |

Schematics themselves are always backed up before a modification; backups live in `backups/` next to the program.

## Textures and the Block Library

The block library and the entity texture library are not shipped with the installer: on first launch, if the library holds no version at all, the program opens the 「版本库」 (Version Library) dialog to guide you through filling it in. It builds one per-version pack per data version and stores them under `base/` in the application data directory (on Windows,
`%APPDATA%\io.github.heiheids.schematic-manager\base`).

There are two sources: **a local scan** (pick any directory and it recursively searches it, subdirectories included, for client jars — anything found is imported automatically and marked ready, with no network traffic) and **online download** (the official endpoints or BMCLAPI, for versions that are neither in the manifest nor installed locally). 49 releases are supported for filling in, from 1.13.2 to 26.3; once a version is built it can be used for block search, 3D preview and statistics, and it can be rebuilt later in the 「版本库」 dialog.

Building a version also collects that version's language files and textures into the library: client jars only carry the English language file, so **Chinese names come from the `.minecraft/assets` resource library**, which means whether Chinese names appear depends on your local resources being complete; when they are missing it falls back to English names. For animated textures only the first frame is taken, as a static display — there is no frame-by-frame animation.

## Tech Stack

| Layer | Choice |
|---|---|
| Desktop framework | Tauri 2 |
| Frontend | Vite + Vue 3 + TypeScript |
| Styling | Sass |
| Component library | Arco Design Vue |
| Backend | Rust (everything except compression — `.litematic`, NBT, ZIP and PNG parsing — is implemented from scratch) |

## Platform Support

| OS | x86_64 | arm64 |
|---|---|---|
| Windows | ✅ | ✅ |
| Linux | ✅ | ✅ |
| macOS | ✅ | ✅ |

## Troubleshooting

**Double-clicked and nothing happened?**

When startup fails, the program writes a `启动失败.log` (startup failure log) in two possible places and shows an error dialog:

1. The program's own directory (by default `%LOCALAPPDATA%\schematic-manager\`)
2. If that is not writable, the system temporary directory

The log contains the failure reason, the error location and the program path.

**"WebView2 error" or "access denied"**

The UI depends on the system's Microsoft Edge WebView2 runtime (bundled with Windows 11). This kind of error usually means its user data directory has been corrupted; delete it and try again:

```
%LOCALAPPDATA%\io.github.heiheids.schematic-manager
```

If it still fails to start, install the WebView2 runtime:
<https://developer.microsoft.com/microsoft-edge/webview2/>

**Unattended environments**

The environment variable `SCHEMATIC_MANAGER_NO_ERROR_DIALOG=1` makes startup failures only write the log without showing the dialog. It is meant for automation scripts or scheduled tasks — the dialog is modal and waits forever when nobody clicks it. It is unset by default, so ordinary users are unaffected.

## Project Layout

- `src/` — Vue frontend
- `src-tauri/` — Rust backend
  - The block library and entity texture library are not shipped: on first run the
    「版本库」 (Version Library) dialog guides you through scanning a local directory
    (subdirectories included; anything found is imported) or downloading client jars
    from the official or BMCL API endpoints, and builds the base library in the app
    data directory
  - `src-tauri/src/` is split into domain packages: `block/` (block library),
    `pack/` (resource packs and base-library building), `format/` (NBT/PNG/ZIP),
    `schematic/` (schematics and block states), `render/` (scene and meshes),
    `app/` (application infrastructure), `mc/` (versions and client downloads) and
    `commands/` (frontend command entry points, DTOs included)
  - `src-tauri/src/tests/` — unit-test package mirroring the same package layout; each
    file is mounted with `#[path]` and runs with `cargo test --lib`
  - `src-tauri/src/tests/data/` — real `.litematic` samples used by tests
- `legacy-cpp/` — the original Windows-only C++ implementation, **kept for reference
  only**. Its semantics for `.litematic` parsing, NBT bit packing, ZIP/PNG decoding and
  multi-version block-library baking have been verified in practice, and served as the
  reference when rewriting in Rust.
- `tools/gen_app_icons.py` — generates the whole app icon set from the two masters in `docs/`
- `tools/typecheck/` — offline type checking and unit tests for environments without network access
- `tools/render/` — renders build artifacts in a headless browser to check the UI really draws
- `docs/` — technical documentation and icon masters
- `.github/workflows/ci.yml` — multi-platform quality checks, automated build and release

## Naming

The installer name, install directory, executable name and bundle identifier all use the ASCII name `schematic-manager` (see `productName` and `identifier` in `src-tauri/tauri.conf.json`); the user-visible display name **投影文件管理器** is specified separately in each place:

- Window title: `app.windows[0].title` in `tauri.conf.json`, with another copy in the title bar
- macOS Finder name and menu-bar name: `src-tauri/Info.plist`,
  `bundle.macOS.bundleName`
- Linux desktop entry name: `src-tauri/linux/main.desktop`
- Startup failure dialog: `src-tauri/src/app/startup.rs`

Windows gets no override: the installer takes the Start Menu and desktop shortcut names as well as the "Apps & features" entry from `productName`, and Tauri has no option that separates them, so those places show `schematic-manager`; the window title and the text inside the UI remain Chinese.

On Linux both deb and rpm use `src-tauri/linux/main.desktop` as the desktop entry template; AppImage has no such template option, so an AppImage build would show `schematic-manager` in the application menu.

Changing the bundle identifier and `productName` together makes the system treat the new version as a different application: on Windows the old installation is no longer detected as upgradable (both the uninstall entry and the install directory changed name), so the old version must be uninstalled first; the application data directory is migrated automatically when the program starts.

## Build

You need Node.js (with pnpm) and a Rust toolchain.

```bash
pnpm install
pnpm tauri dev        # development (dev server + app window)
pnpm app:build        # Windows installer (same as CI: nsis)
pnpm tauri build      # bundle according to targets in tauri.conf.json
```

**Use `pnpm app:build`, not `cargo build --release`.** The latter only compiles Rust and
**does not embed the frontend**: `dist/` is left out, the artifact still contains `devUrl`
(`http://localhost:1420`), and on double-click WebView2 tries to reach a development server
that is not there, so the UI shows `ERR_CONNECTION_REFUSED`. To embed the frontend you must
build through the Tauri CLI, which runs `beforeBuildCommand` (`pnpm build`) before compiling
and bakes `frontendDist` into the binary. This is a pitfall I hit in practice.

Also, `pnpm tauri build` walks every format in `targets: "all"`. MSI used to fail
unconditionally: with a Chinese productName, WiX's light.exe encodes `Product/@Name` using
`SummaryCodepage` (en-US, 1252) and reports `LGHT0311`. Since productName became ASCII, MSI
builds in practice, but the release pipeline does not classify `.msi` artifacts yet, so CI
still uses `nsis` only; for a local artifact matching CI, use `pnpm app:build` (equivalent
to `tauri build --bundles nsis`).

Frontend type checking, unit tests and build:

```bash
pnpm typecheck       # vue-tsc
pnpm test            # vitest: API contract + UI mounting and interaction flows
pnpm check:ipc       # consistency of command names and argument names across the IPC boundary
pnpm check:notes     # Release notes format (asset table and download links)
pnpm check:workflow  # workflow structure: needs / outputs / matrix keys / expressions / concurrency
pnpm build           # vite build
```

`pnpm check:notes` and `pnpm check:workflow` run offline, need no token, and are unaffected
by the repository's current Release state, so you can run them locally at any time.

`check:workflow` covers the gap between "YAML parses" and "GitHub can execute it": `needs`
pointing at a job that does not exist, references to an undeclared output, matrix keys that
are never defined, unbalanced expression braces, and unquoted commas inside the matrix
(which are treated as item separators) — these only surface after a push, and this workflow's
build stage runs on six platforms.

Backend unit tests:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

Unit tests live under `src-tauri/src/tests/`, mirroring the source package layout. Each
test file is the body of a module's `mod tests`, mounted by the
`#[cfg(test)] #[path = "../tests/…"] mod tests;` declaration at the end of that module,
so private items stay reachable and everything runs with `cargo test --lib`.
`support.rs` holds shared helpers (in-memory zip/PNG builders, per-test temp dirs) and
`real_project.rs` is the opt-in end-to-end check.

Only a small sample ships with the source (see `src-tauri/src/tests/data/README.md`).
To check feature coverage against your own real schematics, use these opt-in tests:

```bash
SM_REAL_DIR=/path/to/projects \
  cargo test --manifest-path src-tauri/Cargo.toml --lib -- --ignored --nocapture
```

For every real schematic in the directory they check, in order:

- loading, region structure self-check, lossless NBT round-trip, block counts
- building a 3D scene with the shipped block library and reporting how many blocks failed to parse
- block replacement: replace → write → read back, verifying volume and total block count are unchanged
- version conversion: convert → write → read back, verifying dataVersion was rewritten and the volume is unchanged
- duplicate detection: copies are reported as duplicates and files with different content are not misjudged
- backup and restore: back up → really modify the file → restore, verifying it is byte-for-byte identical again

They fail outright when `SM_REAL_DIR` is unset rather than silently skipping.
They create a working directory under the system temporary directory; if that is not
writable, point `TEMP`/`TMP` at a writable location first (the same applies to
`tools/typecheck/run.ps1`).

UI render check (requires `pnpm build` first, and Chrome / Edge / Chromium installed):

```bash
python tools/render/check-render.py
```

It loads the dist artifacts in a headless browser, automatically walks the real path
"add directory → scan → select file → open each of the five feature dialogs", then checks
the UI text in the rendered output. Unit tests run in jsdom and cannot see rendering in a
real browser: with a wrong build configuration `vite build` still succeeds and dist is still
produced, but opening it shows a blank page. This step guards exactly that case.

The dialogs are checked separately: each loads its own data when opened (convertible
versions, version library manifest, backup list), and if that throws, the UI symptom is
"clicked and nothing happened" — the hardest class of problem to diagnose.
The browser path can be given via `CHROME_PATH`.

## Application Icons

The icons are generated from two 512×512 masters, split by display size: **48 pixels and
below** use `docs/48x.png` (the viewfinder-style fine detail is dropped and the mark is
larger, so it stays legible down to a dozen pixels), and **above 48 pixels** use
`docs/256x.png`.

```bash
python tools/gen_app_icons.py
```

It overwrites the whole set under `src-tauri/icons/` and feeds the two multi-size
containers, `.ico` and `.icns`, per size band — `pnpm tauri icon` only accepts a single
master and cannot do that. After writing, the script reads the containers back and checks
every entry, confirming that each one really came from the master it should have; that step
is not optional, because the band boundary (the 64-pixel entry belongs to the large master)
is exactly where mistakes happen.

The mark in the top-left of the title bar comes from the same script
(`src/assets/app-icon.png`, the small master rendered at 54 pixels) and is used as an image
in the component rather than a hand-written SVG. That keeps the mark and the app icon on one
source: an earlier hand-written SVG was not updated when the icon was redesigned, so the UI
kept showing the old mark.

The 1024 entry is a 2× upscale of the 512 master (there is no larger master), so its edges
are slightly softer than native; with a larger master later, just change `MASTER_LARGE` in
the script.

After changing icons you **must** clear this crate's build-script cache before building:
with only icon files changed, an incremental build does not re-run `build.rs` and the exe
keeps the old icon resources.

```bash
cargo clean -p schematic-manager
```

## Version Numbers

- Development builds (pre-release): `v<MAJOR>.<MINOR>.<PATCH>-dev.<build number>`, generated automatically on every push to main
- Releases: `v<MAJOR>.<MINOR>.<PATCH>`, triggered by pushing a tag with that version; the tag
  must match the version in the repository, otherwise the workflow fails

After a release the workflow automatically bumps the in-repo version to the next PATCH and
pushes it back to main. The bump is based on **the tag that was pushed** and is guarded for
idempotency: when the manifest version is no longer the released version (a re-run, or a
manual bump), it is skipped, so it never bumps twice or goes backwards.

The version appears in four places, all rewritten by `scripts/bump-version.mjs`:
`package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, and this package's own
entry in `src-tauri/Cargo.lock`.

Artifact formats per platform: an NSIS installer (`-setup.exe`) on Windows, a DMG on macOS,
and a deb on Linux.

Pushing to main triggers a multi-platform build published as a pre-release; pull requests
and manual runs only run quality checks and produce no Release. The Release notes (download
table + What's Changed commit list) are generated by `scripts/release-notes-render.mjs`.

## License

Released under the [GNU Affero General Public License v3.0](LICENSE) (AGPL-3.0-only).
If you distribute a modified version — including making it available over a network — you
must publish the corresponding source code.
