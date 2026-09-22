# A worked example of a graphics profile.
#
# The offsets below are made up: they are here to show the shape of a profile,
# not to describe any particular cartridge. To write a real one, run
#
#     cargo run -p zelduh-cli -- rom your-game.gb
#
# which lists the stretches of the file that look most like artwork, then
# render a sheet and work out which tile is which.

name       Example Cartridge

# The original four greens, for anyone who misses the screen.
palette    GRASS 9bbc0f 8bac0f 306230 0f380f
palette    EARTH 9bbc0f 8bac0f 306230 0f380f
palette    STONE 9bbc0f 8bac0f 306230 0f380f

# Pull in two banks worth of tiles.
import     0x30000 256
import     0x34000 128 256

# Ground: one tile repeated across the whole 16x16 square.
terrain    GRASS 16 16 16 16 pal=GRASS
terrain    SAND  17 17 17 17 pal=EARTH

# An object built from four different cells.
terrain    BUSH  32 33 40 41 pal=GRASS

# A wall that mirrors one cell rather than storing four.
terrain    WALL  48 48:x 48:y 48:xy pal=STONE

# The hero, two cells wide and two tall.
sprite     HeroDown0 2 2  64 65 72 73 pal=HERO
sprite     HeroDown1 2 2  66 67 74 75 pal=HERO
sprite     HeroSide0 2 2  68 69 76 77 pal=HERO
