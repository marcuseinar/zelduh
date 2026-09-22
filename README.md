# Zelduh

A randomly generated, multiplayer, top-down action adventure in the shape of the
Game Boy Zelda games. Written in Rust, compiled to WebAssembly, played in a
browser.

```
./build.sh                                  # build the wasm module
cargo run --release -p zelduh-server         # serve the page and host a game
# then open http://localhost:8080
```

For single player, any static server will do:

```
./build.sh && python3 -m http.server -d web 8080
```

## What it is

The screen is the Game Boy's: 160x144 pixels, with a 16-pixel status bar over a
160x128 playfield that holds exactly ten by eight tiles of sixteen pixels. The
camera snaps between rooms rather than scrolling freely. Everything is drawn by
hand into a framebuffer rather than on the GPU, so the pixel grid stays exact.

Every world is generated from a seed, so a number is a shareable place. Put one
in the address bar (`#seed=1234`) or type it into the page.

- **Move** with the arrow keys or WASD
- **A** (sword, or whatever is in the A slot) with Z, J or Space
- **B** (second item) with X or K
- **Swap items** with Enter, **run** with Shift once you find the boots
- A gamepad works; on a touch screen an on-screen pad appears

As a boss: **A** slams everything standing next to you, **B** throws a ring of
fire.

## Milestones

| | |
|---|---|
| Assets loadable from a ROM or other files | done |
| Maps generated randomly | done |
| A player can move around and use items | done |
| Monsters can be fought | done |
| Multiplayer | done |
| Players can play as bosses | done |

## How it is put together

```
crates/
  zelduh-core     the simulation: one frame at a time, from input alone
  zelduh-assets   tiles, palettes, the built-in art, ROM and image importers
  zelduh-gen      world generation: overworlds, dungeons, the graph beneath them
  zelduh-render   the software renderer, 160x144 of packed RGBA
  zelduh-wasm     the browser entry point, a plain C ABI
  zelduh-server   multiplayer: a WebSocket relay with an authoritative clock
  zelduh-cli      a desktop harness: screenshots, map dumps, benchmarks
web/              the page, its script, and the built wasm module
```

### Determinism is the load-bearing decision

`World::step` takes nothing but one button mask per player. There is no
floating point anywhere in the simulation: positions are 24.8 fixed point and
randomness comes from a seeded generator. The same inputs against the same seed
reproduce the same world, bit for bit.

That is what makes multiplayer cheap. The network carries *inputs*, a few bytes
per frame, and every client steps its own copy of the world. Two things keep it
honest:

- Anything other than input that changes the world -- a player joining, leaving,
  or taking over a boss -- travels in the same per-frame message, so it happens
  on an agreed frame on every machine.
- Clients periodically hash their whole world and send it; the server says so
  when someone has drifted, rather than letting two people quietly play
  different games.

Joining a game already in progress works because a world can be serialised
whole: the server hands over the state as it stands, and everything after that
arrives as input.

### The build has no JavaScript toolchain

The wasm module is a plain `cdylib` with a hand-written C ABI rather than
anything a bindings generator produced. `cargo build --target
wasm32-unknown-unknown` is the entire build; there is no npm, no bundler, and
nothing that can fall out of step with the compiler. The page owns input, files,
sound and the canvas; Rust owns the world and the pixels.

### Graphics, and what is not in this repository

The art that ships here is original work in the same four-colour idiom as the
games it is modelled on. **No commercial game data is included, downloaded, or
required.**

An asset pack holds *structure* -- which cells make up a bush, which make up the
hero walking left -- separately from pixels. Importing a tileset therefore
redraws the world without changing how it plays. Pixels can come from:

- a Game Boy ROM **you already own**, read in your browser tab and never
  uploaded,
- a raw 2bpp tile dump (`.chr`, `.2bpp`),
- any image the browser can decode, or a BMP,
- a `.zprofile` text file that says which tiles to import and what to use them
  for.

A cartridge does not record where its graphics live, so a plain ROM gives a
remix rather than a faithful tileset: the importer scores stretches of the file
for structure that looks like artwork and takes the best one. A profile is how
that knowledge gets written down and shared. See
[`docs/profile-format.md`](docs/profile-format.md) and the worked example in
[`assets/example.zprofile`](assets/example.zprofile).

## Development

```
cargo test --workspace          # 231 tests
cargo run -p zelduh-cli -- shot --seed 7 --frames 90 --walk rrrd --out shot.png
cargo run -p zelduh-cli -- sheet --out sheet.png     # every tile and sprite
cargo run -p zelduh-cli -- map --seed 7              # a level as text
cargo run -p zelduh-cli -- bench                     # how fast it runs
cargo run -p zelduh-cli -- rom my-cartridge.gb       # what is inside a ROM
```

The CLI exists because the fastest way to find out that pits were drawing white
is to look at a picture of one.

Measured on the development machine: 72us per simulated frame with four players
and a populated world, 479us to draw one. The wasm module is about 260 KiB.

## Licence

MIT or Apache-2.0, at your option.
