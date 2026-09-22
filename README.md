# Zelduh

A randomly generated, multiplayer, top-down action adventure in the shape of the
Game Boy Zelda games. Written in Rust, compiled to WebAssembly, played in a
browser.

**Play it: https://marcuseinar.github.io/zelduh/**

It works on a phone: put a thumb anywhere on the left of the screen and the
stick appears under it, the picture fills the display in either orientation,
and the page can be added to a home screen and played offline. Every world
comes from a seed, so `#seed=1234` on the end of the address is a place you
can send someone, and `#room=amber-otter-42` is a game you can send them into.

Multiplayer needs nothing running anywhere: players connect directly to each
other, so the link above is the whole thing.

To run it yourself, any static server will do:

```
./build.sh && python3 -m http.server -d web 8080
```

There is also a small server in the repository, for playing together on a
network with no way out to the internet:

```
cargo run --release -p zelduh-server   # serves the page, and introduces peers
```

## What it is

The screen is the Game Boy's: 160x144 pixels, with a 16-pixel status bar over a
160x128 playfield that holds exactly ten by eight tiles of sixteen pixels.
Everything is drawn by hand into a framebuffer rather than on the GPU, so the
pixel grid stays exact.

The world is continuous. The camera follows you across one unbroken map
instead of jumping a screenful at a time, which means a phone can be shown
more of the world rather than black bars: **Fill the screen** picks a
viewport the shape of your display, keeping the pixels the same size. What
you can see is a local choice and never reaches the simulation, so two
players on differently shaped screens stay exactly in step.

Rooms still exist. They are what generation lays out, and inside a dungeon a
monster belongs to one room and stays in it — a doorway is for heroes.

Every world is generated from a seed, so a number is a shareable place. Put one
in the address bar (`#seed=1234`) or type it into the page.

- **On a phone**, put a thumb down anywhere on the left of the screen and the
  stick appears under it: no pad to find, diagonals included, and you can
  slide between directions without lifting off. **FULL** goes fullscreen,
  **SWAP** exchanges your A and B items, and **MENU** opens the settings.
- **Move** with the arrow keys or WASD
- **A** (sword, or whatever is in the A slot) with Z, J or Space
- **B** (second item) with X or K
- **Swap items** with Enter, **run** with Shift once you find the boots
- A gamepad works; on a touch screen an on-screen pad appears

As a boss: **A** slams everything standing next to you, **B** throws a ring of
fire.

## Deploying

The site is static -- an HTML page, a script, a stylesheet and a wasm module --
so GitHub Pages serves it as-is. The workflow in
[`.github/workflows/pages.yml`](.github/workflows/pages.yml) builds the module
and publishes `web/` on every push.

It needs one setting turned on once, in **Settings -> Pages -> Build and
deployment -> Source: GitHub Actions**. Until that is done the workflow will
run and fail at the deploy step.

Multiplayer works from static hosting too, because there is nothing for a
server to do: peers connect straight to each other. Finding each other in the
first place is the one thing that needs somebody else's help, and by default
that is a public relay -- pick one under "Find peers via" in the "Play
together" box.

If you would rather not use one, `zelduh-server` does the same job on your own
machine: run it, choose **Your own server**, and put its `ws://` address in the
relay box. A page served over https can only open `wss://`, so a self-hosted
relay used from the published site needs to be behind TLS; a page you are
serving yourself over http has no such problem.

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
  zelduh-server   optional signalling for peer-to-peer play, and a file server
  zelduh-cli      a desktop harness: screenshots, map dumps, benchmarks
web/              the page, its script, and the built wasm module
```

### Determinism is the load-bearing decision

`World::step` takes nothing but one button mask per player. There is no
floating point anywhere in the simulation: positions are 24.8 fixed point and
randomness comes from a seeded generator. The same inputs against the same seed
reproduce the same world, bit for bit.

That is what makes multiplayer cheap, and what makes it possible without a
server at all. Nobody sends a position or a health bar. The network carries
*inputs* -- a couple of bytes per player per frame -- and every peer steps its
own copy of the world.

The model is lockstep with delayed input: a peer sends its buttons a few frames
ahead of the frame it is simulating, and simulates a frame once everybody's
buttons for it have arrived. Three things keep it honest:

- One peer, whichever has the lowest id, acts as the arbiter. It hands out
  player slots and stamps joins, departures and boss takeovers with the frame
  they take effect on, so the shape of the game changes on the same frame
  everywhere. It holds no authority over the simulation itself, and if it
  disappears the next-lowest id takes over without anything else changing.
- Peers periodically hash their whole world and swap the hashes, so a
  disagreement is reported rather than quietly played out as two different
  games.
- WebRTC can take the better part of a minute to admit a connection has died,
  so a peer that holds everyone up for more than a few seconds is written off
  rather than waited for.

Joining a game already in progress works because a world can be serialised
whole: the arbiter hands over the state as it stands, and everything after that
arrives as input.

### The build has no JavaScript toolchain

The wasm module is a plain `cdylib` with a hand-written C ABI rather than
anything a bindings generator produced. `cargo build --target
wasm32-unknown-unknown` is the entire build; there is no npm, no bundler, and
nothing that can fall out of step with the compiler. The page owns input, files,
sound and the canvas; Rust owns the world and the pixels.

Two jobs are not worth doing by hand, and those are somebody else's libraries,
checked into [`web/vendor`](web/vendor) as their published builds rather than
installed: [nipplejs](https://github.com/yoannmoinet/nipplejs) draws the thumb
stick, and [Trystero](https://github.com/dmotz/trystero) introduces peers to
each other over WebRTC. Both are MIT licensed. Vendoring them keeps the build
`cargo build` and the page working offline, with no CDN in the critical path.

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
