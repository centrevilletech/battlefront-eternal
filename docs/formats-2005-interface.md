# 2005 interface formats: the loading screen and the HUD definitions

Layout and formats only, read from a Steam copy of the 2005 game. This
file does not contain retail data. The readers are
`tools/zero_load2.py` and `tools/zero_hud2.py`; the plan that uses them
is `docs/battlefront-ii.md` (decisions 5 and 6, risk 5).

## The shared config shape

Both files are config chunks, the `DATA`/`SCOP` trees of
`docs/formats.md` ("Sky and effects config"), with one addition: the
chunk body starts with a `NAME` chunk holding one u32, the FNV-1a hash
(`tools/zero_hash.py`) of the file's name, then `DATA` chunks each
followed by an optional `SCOP` with that entry's children.

A `DATA` chunk is the hash of the field name (u32), a value count (u8),
that many 4-byte values, and a string table (u32 size, then the
strings). A value is a float, a name hash, or an offset into the string
table: the string sits at values start + offset + 4, so string offsets
start at 4 * count and the referenced strings (with their terminators)
tile the table exactly. `zero_hud2` reads this strictly: a chunk that
parses has every byte accounted for. The reader checks the enclosing
archive sizes, zero alignment, string starts and root identity. Duplicate
file names, truncated chunks and excess nesting or node counts fail the
preparation step before it publishes the trees.

All 193 field and file hashes in `zero_hud2.NAMES` occur in the stock
chunks. Of their spellings, 172 match the swbf-unmunge dictionary exactly
and 20 match after case folding. `LoadDisplay` comes from the project's
2004 reader in `tools/zero_shell.py`. See THIRD-PARTY-NOTICES.md.

## The loading screen (`load` in `load/load.lvl`)

One chunk, named `load`. Its entries:

- `LoadDisplay` (no values), the shared section. Its children:
  - `LoadingTextBlinkRate` (1 float) and `LoadingTextColorPallete`
    (no values) with `color` children (4 numbers each, 0..255 with
    alpha): the loading text's blink and its two colours.
  - `TipsBoxTexture` (texture name, then 2 numbers): the tips box's
    nine-slice art and its border sizes. The texture (128 x 128,
    DXT3) is in the same archive.
  - `TipsColor` (4 numbers): the tip text's colour.
  - `TipsPrefix` (a localization key prefix) and `TipsTime` (a
    duration in seconds, then one name-hash word whose name is not
    known; it is stored raw). The tips are the `Locl` keys
    `<prefix>.1`, `<prefix>.2`, ... in `core.lvl`, numbered from 1
    with no gaps (54 under the default prefix in the copy read).
  - The progress bar: `ProgressBarLEDTexture` (a 32 x 32 DXT3 texture
    in the same archive), `ProgressBarNumLEDs`,
    `ProgressBarLEDWidth`, `ProgressBarLEDHeight`,
    `ProgressBarLEDSpacing`, `ProgressBarLEDColor` (4 numbers),
    `ProgressBarLEDMinIntensity` (a fraction, then the same unknown
    hash word as `TipsTime`), and `ProgressBarTotalTime` (seconds for
    the bar to fill; the default when a world names none).
  - `RandomBackdrop` (a path like `load\common`): the default picture
    archive. `AmbientColor`, `SunColor` (3 numbers each) and
    `SunDirection` (3 numbers) light whatever the screen draws in 3D;
    the loading backdrop uses none of them.
- One `map` entry per world (its value is the world id), with
  `RandomBackdrop` (that world's archive under `load/`) and usually
  `ProgressBarTotalTime`. Campaign worlds add `CampaignLayout` (a
  number) and `CampaignName` (a localization key); the Galactic
  Conquest worlds override `TipsPrefix` (21 tips under theirs).

The picture archives (`load/<stem>.lvl`) hold one `tex_` per picture,
1024 x 1024 DXT1, named with a numeric suffix (`mos_1`..`mos_4` in
Mos Eisley's, `spa_1`..`spa_13` in the space maps' shared archive,
one `loadscreen` in `load/common.lvl`). The PC screen draws one candidate
at random and holds it for the load. The tip interval and map duration
come from the config. `load/shell.lvl` is an 8-byte stub with
nothing in it.

Prepared form (`zero_load2.write_loading_screen`):
`common/shell/load2.json` (the display section, the tips with their
text grouped once by language and referenced prefix, and each world's
archive stem, pictures, tip prefix, campaign fields and `durationTicks`) and the pictures as PNGs under
`common/shell/load/`, through the same texture path as the 2004 shell
art. Each tip includes exclusive UTF-8 line ends measured with the copy's
small bitmap font. A 400-unit panel reserves at least 18 units per side
for the border, shadow and text inset. Wider borders reduce the wrap width. The display
records `tipTicks` and the largest line count across all languages and worlds. Each language also has map titles and a loading
label. Each PNG path includes its source archive name and digest. Preparation
regenerates and atomically replaces each image, then atomically replaces
the manifest. A changed archive uses fresh paths, so an interruption leaves
any previous manifest referring to its own complete pictures. The loading
reader limits each texture to 4096 per edge and four megapixels, checks
the encoded top mip before decoding it, and only expands referenced
pictures. The prepared manifest is limited to 4 MiB; a map can name up
to 256 pictures, a group 512 tips, and a bar 64 LEDs.

## The HUD definitions (`hud_` in `ingame.lvl`)

Five chunks: `hudtransforms` and one per split-screen player count
(`1playerhud` .. `4playerhud`; the 3-player file is a stub holding one
mask bitmap). Each starts with `FileInfo` (the file name) whose child
`Viewports` lists the player viewports it serves, then the element
forest.

An element is an entry with a scope, including an empty scope: its field name is the element
type, its single string value the instance name, and its children are
properties (entries without a scope) and child elements, in file
order. `1playerhud` holds 322 scoped entries of 77 types and 1,793
leaves of 81 kinds. These include 98 per-mesh records, 52 text entries,
41 groups, 21 bitmap scopes and 16 model entries, plus bar and marker
templates. The split-screen files describe viewport-specific layouts.
The common types:

| Type | What it is |
| --- | --- |
| `group` | A container: placement, fade and enable state for its children |
| `text` | A text label (`TextFont`, `TextStyle`, `TextBreak`, `TextBox`, `TextAlignment`, ...) |
| `Bitmap` | A textured quad; also a property naming another element's texture |
| `Model3D` | A live 3D model on the HUD (the held weapon), with `Mesh` or `EventMesh` |
| `MeshInfo` | Per-mesh placement overrides inside a `Model3D` (one per weapon mesh) |
| `BarBitmap`, `BarSegmented`, `Segment` | Fill bars and their segments |
| `BorderedBox`, `MultilineText`, `format`, `ProceduralBarBitmap`, `VehicleSeating`, `ObjectiveList`, `Empty` | The remaining widgets |
| `map`, `target` | The minimap and the target-marker root; their children name a template per marker kind and screen state (on-screen, behind, off-screen, per map size) |
| `Viewport` | The transforms file's root container |

Properties place and style an element in a normalized space:
`position` (x, y, z, space name), `Scale` and `rotation` (3 numbers),
`BitmapRect`/`Rect` (size, then anchors), `texcoords` (4 numbers),
`color` (3 numbers) and `alpha`, `ZOrder`, `Viewport` (which player's
viewport), fade times (`FadeInTime`, `FadeHoldTime`, `fadeouttime`,
`FadeSustainTime`), pulse and colour-change rates, and text settings.
A property may be absent. Of 52 one-player text entries, 47 specify a
position and 49 a font. Seven of 16 models specify a position. Four
distinct referenced fonts resolve in the five-font core archive. The
interpreter must apply the appropriate defaults and inherited properties.

Bindings include both values and signals. Of 176 distinct one-player
event strings, eight are undotted signals such as `initialize`.
`EventEnable` and `EventDisable` govern visibility. Other bindings feed
text, numbers, colour, meshes, positions, rotations, scale or alpha.

`hudtransforms` holds eight colour ramps, four vector ramps and two
name-to-mesh maps. Each has input and output bindings. Their stops and
pairs are data. The single name filter is on the enclosing viewport.

Prepared form (`zero_hud2.write_hud_files`): one JSON element tree per
chunk under `common/hud/` (`name`, `hash`, `values`, `children` per
entry; `children` is absent on leaves and empty for empty scopes. Float
values round-trip to their stored words, including signed zero. Other raw
words are recorded as `{"bits", "name"}`).

## What this settles for the HUD (plan risk 5)

A HUD interpreter can use the authored viewport trees, explicit widget
properties, bar geometry, mesh overrides, marker templates, transform
tables and bindings. The runtime still supplies defaults and inherited
values, publishes game state, dispatches signals, evaluates transforms,
projects markers, and renders widgets, meshes and text.

The PC battle presentation catalog, written by `tools/zero_battle_ui2.py`,
is `common/hud/presentation.json`. Its trees and PNGs point into
archive-qualified directories. A flat equipment mesh can name its diffuse
texture in segment texture slot zero. The catalog preserves that association.
Each installed language has its own string table indexed by hexadecimal
localization ID. Preparation publishes the catalog after all referenced files.


### Battle presentation catalog

`zero_battle_ui2.write_battle_interface` publishes `common/hud/presentation.json` after its referenced files. It maps definition names and texture names to archive-qualified files, mesh names to their projected icons, and installed languages to complete localization tables indexed by eight hexadecimal digits. Small HUD meshes retain their outline, diffuse art and vertex alpha when projected onto transparent equipment and marker images. `bounds` gives, for each projected icon, the model's extent on the two axes the picture shows, in model units, as left, bottom, right and top (the picture's vertical axis points up and keeps a one pixel margin). The per-map HUD catalogs (`maps/<map>/hud/catalog.json`) carry the same `bounds` for the class and weapon models they project. They also publish every texture of the map's archives that no model draws and the world does not take: the art a battle's scripts name at run time, such as Space Assault's ship, shield and critical-system pictures (generation 11). The catalog belongs inside the prepared 2005 content root.

The single-viewport reader composes nested pixel or viewport positions and scales in a 1024 by 768 reference plane. A group, objective list or bordered box with a `Rect` places its children from the rect's aligned corner. A group's colour tints what it encloses, and alpha multiplies down the tree. Bitmap rectangles carry alignment and texture coordinates; text widgets carry their text box, horizontal and vertical alignment and shadow style; a model widget's `MeshInfo` children give each mesh's placement inside it. Number-to-colour transforms interpolate sorted stops, and equipment bindings choose a mesh from the current HUD tag. The localizer tries the requested and base languages, English and the other installed tables for each missing string.

The battle HUD draws a HUD model's picture with the model's origin at the widget and one model unit spanning a fifth of the plane's height per unit of `Scale`, text in the definition's own bitmap fonts, and pins each top-level group to a screen edge: a group whose position lies in the left or right third of the plane follows that edge at the plane's size, the others stay centred, and the same thirds pin it to the top, the middle or the bottom. Elements that follow a projected point, such as the reticle and lock brackets, follow it across the whole window.
