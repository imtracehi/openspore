# OpenSpore

OpenSpore is an open-source re-creation of the game engine of Spore (2008),
written in Rust with the [Bevy](https://bevyengine.org/) engine. Like
[OpenRCT2](https://openrct2.io/), it contains **no game files**: it plays with
the files of your own installed copy of Spore, which it only reads and never
changes.

**Status: early test builds.** All five stages (Cell, Creature, Tribal,
Civilization, Space), the editors and the menus are playable. The goal is to
match the original game in visuals, gameplay, menus and audio; expect bugs and
differences from the original. Reports help a lot (see Feedback below).

OpenSpore is not made or endorsed by Maxis or Electronic Arts.

## Play

**Requirements:** Windows 10 or 11 (64-bit) with a Vulkan-capable graphics
card, and an installed copy of Spore (Steam, EA app / Origin, GOG or disc).
Galactic Adventures and the parts packs are used when installed.

**Install:** download `OpenSpore-<version>-setup.exe` (recommended) or the
portable zip from [Releases](../../releases).

- The setup needs no administrator rights. Its "Your Spore installation" page
  shows the Spore folder it found; check it (or click Browse) and continue.
- The portable zip runs from any folder and keeps its settings and saves next
  to `openspore.exe`.
- The downloads are not code-signed yet, so Windows SmartScreen may show
  "Windows protected your PC": choose More info, then Run anyway.

The Spore folder is the one that contains the `Data` folder, for example
`...\Steam\steamapps\common\Spore` or `C:\Program Files (x86)\Electronic Arts\SPORE`.
If OpenSpore cannot find Spore, it asks for the folder when it starts. To pick
another folder later, use the Start Menu entry "OpenSpore - change Spore
folder" (or run `openspore.exe --choose-spore`).

**Your files:** settings and saves live in `%APPDATA%\OpenSpore`, cached files
in `%LOCALAPPDATA%\OpenSpore` (portable mode: next to the exe). Uninstalling
asks before deleting your saves and never touches your Spore installation.

## Feedback

Please report problems in [Issues](../../issues/new/choose), using one of two
forms:

- **Bug report**: a crash, a freeze, or something that does not work.
- **Difference from the original Spore**: something that works, but looks,
  sounds or plays differently from the original game.

The forms ask for the details that help most: your OpenSpore version, Windows
version, graphics card, which Spore you own, and the text in OpenSpore's
console window. If OpenSpore cannot find your Spore install, include the output
of `openspore.exe --detect-spore`.

## Command line

| Option | What it does |
|---|---|
| `--spore-path <folder>` | Use this Spore folder for this run |
| `--choose-spore` | Pick the Spore folder again (it is remembered) |
| `--detect-spore` | Print where OpenSpore looks for Spore and what it found |
| `--save-path <file>` | Use this save file instead of the default |
| `--no-dialogs` | Never show the folder picker (print an error instead) |

The environment variable `OPENSPORE_SPORE_PATH` works like `--spore-path`. A
file named `portable.txt` next to `openspore.exe` turns on portable mode.

## Build from source

You need Windows 10 / 11 (64-bit), [Rust](https://rustup.rs/) (stable, MSVC
toolchain) and an installed copy of Spore.

```powershell
cargo build --release
target\release\openspore.exe
```

Tests: `cargo test --bin openspore` runs the tests that need no Spore install.
The tests that read your install are ignored by default; run them with
`cargo test --bin openspore -- --ignored`.

Release packaging: `installer\build-release.ps1` builds the release exe and
stages the portable zip, and compiles the setup with
[Inno Setup 6](https://jrsoftware.org/isinfo.php) when it is installed. The
GitHub workflow in `.github\workflows\release.yml` does the same on a `v*` tag
and attaches the files to a Release.

## Legal

Spore is a trademark of Electronic Arts Inc. You must own a copy of Spore to
play OpenSpore. This repository and its releases contain no Spore game files;
please do not add any (packages, or textures, sounds or models extracted from
them) in issues or pull requests.

## License

OpenSpore is free software under the GNU General Public License, version 3 or
later. See [LICENSE](LICENSE).
