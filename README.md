# ExeyVue

**Fast small images viewer and editor.**

A minimal, liquid-glass image viewer written in Rust with [iced](https://iced.rs).
It opens PNG, JPEG, GIF (animated) and JPEG XL, and ships two editing tools to start
with: a **Knife** that splits an image, and **Merge** that puts pieces back together in
any order and direction.

<!-- Screenshot: docs/screenshot.png -->

## Features

- **View** — zoom with the scroll wheel, drag to pan, `←` `→` to walk through the folder,
  drag & drop files onto the window, open from the command line, Finder / Explorer
  "Open With", or by dropping a file on the Dock icon.
- **Formats** — PNG, JPEG, GIF (animated GIFs play), JPEG XL (`.jxl`) read *and* write.
- **Knife** — one straight cut, horizontal or vertical. The knife line follows the mouse,
  snaps to the exact pixel boundary and comes with a pixel magnifier; click to cut.
  Pieces land in a tray and can be re-ordered.
- **Merge** — combine tray pieces **Horizontally**, **Vertically** (default) or in a **Grid**,
  with *Fill to largest* on or off. The result becomes the current image; Undo goes back.
- **Save** — export the current image as PNG, JPEG, GIF or JPEG XL.
- **Glass** — a slider in the toolbar sets how see-through the window is.

## Download

Pre-built binaries are attached to every [GitHub release](https://github.com/Exey/ExeyVue/releases):

| Platform | File |
| --- | --- |
| macOS (Apple Silicon) | `ExeyVue-macos-arm64.zip` — contains `ExeyVue.app` |
| Windows (x64) | `ExeyVue-windows-x64.zip` — contains `ExeyVue.exe` |
| Linux (x64) | `ExeyVue-linux-x64.tar.gz` — contains `exeyvue` + a `.desktop` file |

The macOS app is ad-hoc signed, not notarised, so the first launch needs a
right-click → **Open**, or:

```sh
xattr -dr com.apple.quarantine ExeyVue.app
```

Every push to `main` also builds all three platforms; the results are available as
workflow artifacts on the [Actions](https://github.com/Exey/ExeyVue/actions) page.

## Using ExeyVue

```sh
exeyvue photo.png        # opens the file and indexes its folder for ← → browsing
```

| Key | Action |
| --- | --- |
| `←` / `→` | Previous / next image in the folder |
| `Esc` | Back to View mode |
| `K` | Knife mode |
| `M` | Merge mode |
| `H` / `V` | Horizontal / vertical knife line |
| `Ctrl`/`⌘` + `O` | Open… |
| `Ctrl`/`⌘` + `S` | Save… |
| `Ctrl`/`⌘` + `Z` | Undo the last merge / tray open |

### Knife

Switch to **Knife** and move the mouse over the image. The cut line follows it and snaps
to the nearest pixel boundary. It is drawn as three pixel-thin lines — one on the boundary
and one a pixel away on each side — in *difference* colours (the image inverted), so it
stays visible on any background and the image shows through the 1 px gaps.

A magnifier rides along the line with the mouse: two 6×3-pixel blocks, one on each side
of the cut, with the cut running through the gap between them. The upper (or left) block
shows the last three rows (columns) that end up in the first piece, the lower (right)
block the first three of the second piece — so you can see exactly which pixels go where.
The pixel coordinate is shown next to it.

Click to cut. The two pieces (`name·L` / `name·R`, or `name·T` / `name·B`) are added to
the tray at the bottom. Click a tray thumbnail to make that piece the current image and
cut it again — repeated cuts give you as many pieces as you need. The original stays
untouched until you save.

### Merge

The tray is the merge input, in the order shown; use `<` `>` under each thumbnail to
re-order and `x` to drop a piece. You can also add other files with **+ Files…** or the
current image with **+ Current**, and drop files onto the window while in Merge mode.

| Direction | Layout | *Fill to largest* on | *Fill to largest* off |
| --- | --- | --- | --- |
| Horizontal | side by side | every piece scaled to the tallest height | smaller pieces centred vertically on transparent padding |
| Vertical (default) | stacked | every piece scaled to the widest width | smaller pieces centred horizontally on transparent padding |
| Grid | rows × columns | every piece scaled to fit the largest cell | smaller pieces centred in their cell |

Scaling always keeps the aspect ratio (Lanczos3). Grid columns default to
`ceil(sqrt(n))`; set them explicitly with the slider. Padding is transparent, so save as
PNG or JXL to keep it, or JPEG to flatten it onto white.

Animated GIFs play in View mode; the edit tools work on the first frame.

## Building from source

Requires a recent stable Rust toolchain (`rustup`).

```sh
git clone https://github.com/Exey/ExeyVue.git
cd ExeyVue
./run_dev.sh path/to/image.png        # debug build + run
cargo run --release -- path/to/image.png
```

`run_dev.sh` accepts `NO_JXL=1` (skip the libjxl build, much faster first compile),
`CHECK=1` (type-check only) and `CARGO_FLAGS="…"` for anything else.

JPEG XL support is on by default and compiles `libjxl` from source, which needs
**CMake** and a C++ compiler:

| OS | Prerequisites |
| --- | --- |
| macOS | `xcode-select --install`, `brew install cmake` |
| Windows | Visual Studio Build Tools (C++ workload) and CMake |
| Linux (Debian/Ubuntu) | `sudo apt install cmake ninja-build pkg-config libgtk-3-dev libxkbcommon-dev libwayland-dev` |

Run the tests for the knife/merge logic with `cargo test`.

### Project layout

```
src/main.rs      application state, messages, update loop, UI
src/formats.rs   decoding/encoding (image crate + libjxl)
src/ops.rs       knife + merge, pure functions with unit tests
src/knife.rs     canvas overlay: cut lines + pixel magnifier
src/style.rs     liquid-glass styling
src/macos.rs     Finder / Dock "open file" events (macOS only)
run_dev.sh       debug build + run helper
packaging/       macOS Info.plist, Linux .desktop
.github/         cross-platform build + release workflow
```

### Releasing

Tag a commit and push the tag; the workflow builds all three platforms and attaches
the archives to a GitHub release:

```sh
git tag v0.1.0
git push origin v0.1.0
```

## Roadmap

- Crop, rotate, resize
- Several cuts in one pass and free-angle cuts
- Encoder settings when saving (JPEG quality, JXL distance/lossless)
- Editing animated GIFs frame by frame
- Fully custom window chrome on all platforms

## License

[MIT](LICENSE)
