# The `.zprofile` format

A profile says where a Game Boy ROM keeps its graphics and what each tile
should be used for. It exists because nothing in a cartridge records that:
pointed at a plain ROM, the engine can only guess which stretches hold artwork,
which gives an interesting remix rather than a faithful tileset.

A profile is plain text. Blank lines are ignored, and `#` starts a comment. One
bad line is reported and skipped rather than sinking the whole file.

## Directives

### `name <text>`

Names the pack. Shown in the page's status line.

### `import <offset> <count> [base]`

Reads `count` 2bpp tiles from a byte offset in the ROM and puts them in the
pack starting at tile index `base` (0 by default). Offsets may be decimal or
hex with a `0x` prefix.

```
import 0x30000 256        # 256 tiles from bank 12
import 0x34000 64 256     # 64 more, placed after them
```

Use `zelduh rom <file>` to list the stretches of a ROM that score highest as
artwork; those offsets are a good place to start looking.

### `palette <slot> <c0> <c1> <c2> <c3>`

Sets one of the sixteen palettes. Colours are `rrggbb` hex. Slots may be named
(`GRASS`, `EARTH`, `WATER`, `STONE`, `DUNGEON`, `HERO`, `ENEMY_RED`,
`ENEMY_BLUE`, `ENEMY_GREEN`, `BONE`, `GOLD`, `HEART`, `HUD`, `SHADE`, `FIRE`,
`BOSS`) or given as a number.

```
palette GRASS 9bbc0f 8bac0f 306230 0f380f     # the original green screen
```

In a sprite palette, colour 0 is transparent.

### `terrain <name> <c0> <c1> <c2> <c3> [pal=NAME]`

Binds a 16x16 terrain tile to four 8x8 cells, in reading order: top-left,
top-right, bottom-left, bottom-right. Names are the tile constants: `GRASS`,
`BUSH`, `WATER`, `WALL_DUNGEON`, `DOOR_LOCKED` and so on.

```
terrain BUSH 240 241 248 249 pal=GRASS
```

A cell is a tile index, optionally with flips (`241:x`, `241:y`, `241:xy`), or
`-` for nothing.

`pal=` may be left off. Then each cell keeps the palette its tile was fitted
to when the image was imported, which is what you want for art that came from
a picture: the quantiser has already worked out that this cell is grass and
that one is a path. Name a palette when the art came from a ROM, where the
tiles are two bits per pixel and carry no colour of their own.

### `sprite <name> <cols> <rows> <cells...> [pal=NAME]`

Binds a sprite to `cols * rows` cells in reading order. Sprite names are the
`SpriteId` variants: `HeroDown0`, `Octorok1`, `Boss0`, `IconSword`, and the
rest. `zelduh sheet` renders them all with their numbers.

```
sprite HeroDown0 2 2  40 41 48 49 pal=HERO
sprite HeroSide0 2 2  44 45 52 53 pal=HERO
```

## Using one

Drop the ROM on the page first, then the profile: the import directives need
the cartridge's bytes, and the bindings are applied on top.
