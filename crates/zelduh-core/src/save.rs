//! Snapshots: turning a whole world into bytes and back.
//!
//! This is what lets someone join a game already in progress. It is also the
//! safety net under lockstep multiplayer: every machine can hash its own state
//! with [`World::checksum`], and a mismatch says the simulations have drifted
//! apart long before anyone notices monsters standing in different places.

use crate::entity::{Entities, Entity, EntityId, Kind};
use crate::event::Events;
use crate::geom::{Dir, V2};
use crate::input::Input;
use crate::items::{Inventory, Item};
use crate::level::{Level, LevelKind, Link, Map, Room, RoomKind, Spawn};
use crate::rng::Rng;
use crate::world::{Camera, Player, Role, World};

/// Magic bytes at the head of every snapshot.
const MAGIC: &[u8; 4] = b"ZDUH";
/// Snapshot format version. Bump when the layout changes.
const VERSION: u16 = 1;

/// Appends primitives to a byte buffer, little endian throughout.
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Writer {
        Writer { buf: Vec::new() }
    }

    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub fn bool(&mut self, v: bool) {
        self.buf.push(u8::from(v));
    }

    pub fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn i16(&mut self, v: i16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn bytes(&mut self, v: &[u8]) {
        self.u32(v.len() as u32);
        self.buf.extend_from_slice(v);
    }

    pub fn vec2(&mut self, v: V2) {
        self.i32(v.x);
        self.i32(v.y);
    }

    pub fn finish(self) -> Vec<u8> {
        self.buf
    }
}

impl Default for Writer {
    fn default() -> Self {
        Writer::new()
    }
}

/// Reads primitives back. Every read is fallible, so a truncated or hostile
/// snapshot returns `None` rather than panicking.
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Reader<'a> {
        Reader { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let slice = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }

    pub fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|s| s[0])
    }

    pub fn bool(&mut self) -> Option<bool> {
        self.u8().map(|v| v != 0)
    }

    pub fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|s| u16::from_le_bytes([s[0], s[1]]))
    }

    pub fn i16(&mut self) -> Option<i16> {
        self.u16().map(|v| v as i16)
    }

    pub fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    pub fn i32(&mut self) -> Option<i32> {
        self.u32().map(|v| v as i32)
    }

    pub fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|s| {
            u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]])
        })
    }

    pub fn bytes(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }

    pub fn vec2(&mut self) -> Option<V2> {
        Some(V2::new(self.i32()?, self.i32()?))
    }

    /// True when everything has been consumed.
    pub fn at_end(&self) -> bool {
        self.pos >= self.data.len()
    }
}

fn write_entity(w: &mut Writer, e: &Entity) {
    w.u8(e.kind as u8);
    w.bool(e.alive);
    w.u16(e.generation());
    w.u16(e.level);
    w.vec2(e.pos);
    w.vec2(e.vel);
    w.i32(e.z);
    w.i32(e.vz);
    w.u8(e.dir as u8);
    w.i16(e.hp);
    w.i16(e.max_hp);
    w.vec2(e.knock);
    w.u8(e.knock_frames);
    w.u8(e.iframes);
    w.u8(e.stun);
    w.u8(e.state);
    w.u16(e.timer);
    w.u8(e.anim);
    w.u8(e.anim_timer);
    w.u32(e.flags);
    for v in e.data {
        w.i32(v);
    }
    w.u16(e.owner.idx);
    w.u16(e.owner.gen);
    w.u8(e.player);
    w.i32(e.body_w);
    w.i32(e.body_h);
}

fn read_entity(r: &mut Reader) -> Option<Entity> {
    let mut e = Entity::default();
    e.kind = Kind::from_u8(r.u8()?);
    e.alive = r.bool()?;
    e.set_generation(r.u16()?);
    e.level = r.u16()?;
    e.pos = r.vec2()?;
    e.vel = r.vec2()?;
    e.z = r.i32()?;
    e.vz = r.i32()?;
    e.dir = Dir::from_u8(r.u8()?);
    e.hp = r.i16()?;
    e.max_hp = r.i16()?;
    e.knock = r.vec2()?;
    e.knock_frames = r.u8()?;
    e.iframes = r.u8()?;
    e.stun = r.u8()?;
    e.state = r.u8()?;
    e.timer = r.u16()?;
    e.anim = r.u8()?;
    e.anim_timer = r.u8()?;
    e.flags = r.u32()?;
    for i in 0..4 {
        e.data[i] = r.i32()?;
    }
    e.owner = EntityId {
        idx: r.u16()?,
        gen: r.u16()?,
    };
    e.player = r.u8()?;
    e.body_w = r.i32()?;
    e.body_h = r.i32()?;
    Some(e)
}

fn write_level(w: &mut Writer, l: &Level) {
    w.u8(l.kind as u8);
    w.u8(l.dungeon);
    w.u8(l.ground);
    w.i32(l.map.rooms_w);
    w.i32(l.map.rooms_h);
    w.bytes(l.map.raw());
    w.vec2(l.entrance);

    w.u32(l.rooms.len() as u32);
    for room in &l.rooms {
        w.u8(room.kind as u8);
        w.u8(room.region);
        w.bool(room.visited);
        w.bool(room.cleared);
        w.u8(room.exits);
    }

    w.u32(l.links.len() as u32);
    for link in &l.links {
        w.i32(link.from.0);
        w.i32(link.from.1);
        w.u16(link.to_level);
        w.vec2(link.to_pos);
        w.u8(link.to_dir as u8);
    }

    w.u32(l.spawns.len() as u32);
    for s in &l.spawns {
        w.u16(s.room);
        w.u8(s.kind);
        w.i32(s.tx);
        w.i32(s.ty);
        w.i32(s.param);
    }
}

fn read_level(r: &mut Reader) -> Option<Level> {
    let kind = match r.u8()? {
        1 => LevelKind::Dungeon,
        2 => LevelKind::Cave,
        3 => LevelKind::Interior,
        _ => LevelKind::Overworld,
    };
    let dungeon = r.u8()?;
    let ground = r.u8()?;
    let rooms_w = r.i32()?;
    let rooms_h = r.i32()?;
    let tiles = r.bytes()?;
    let entrance = r.vec2()?;

    let mut map = Map::new(rooms_w, rooms_h, 0);
    if tiles.len() != map.raw().len() {
        return None;
    }
    for (i, t) in tiles.iter().enumerate() {
        let w = map.w();
        map.set(i as i32 % w, i as i32 / w, *t);
    }

    let room_count = r.u32()? as usize;
    // A snapshot claiming more rooms than the map has is malformed.
    if room_count != (rooms_w.max(1) * rooms_h.max(1)) as usize {
        return None;
    }
    let mut rooms = Vec::with_capacity(room_count);
    for _ in 0..room_count {
        rooms.push(Room {
            kind: match r.u8()? {
                1 => RoomKind::Start,
                2 => RoomKind::Treasure,
                3 => RoomKind::Boss,
                4 => RoomKind::Shop,
                5 => RoomKind::Puzzle,
                6 => RoomKind::Corridor,
                7 => RoomKind::Empty,
                _ => RoomKind::Normal,
            },
            region: r.u8()?,
            visited: r.bool()?,
            cleared: r.bool()?,
            exits: r.u8()?,
        });
    }

    let link_count = r.u32()? as usize;
    let mut links = Vec::with_capacity(link_count.min(4096));
    for _ in 0..link_count {
        links.push(Link {
            from: (r.i32()?, r.i32()?),
            to_level: r.u16()?,
            to_pos: r.vec2()?,
            to_dir: Dir::from_u8(r.u8()?),
        });
    }

    let spawn_count = r.u32()? as usize;
    let mut spawns = Vec::with_capacity(spawn_count.min(65536));
    for _ in 0..spawn_count {
        spawns.push(Spawn {
            room: r.u16()?,
            kind: r.u8()?,
            tx: r.i32()?,
            ty: r.i32()?,
            param: r.i32()?,
        });
    }

    Some(Level {
        kind,
        map,
        rooms,
        links,
        spawns,
        entrance,
        dungeon,
        ground,
    })
}

fn write_player(w: &mut Writer, p: &Player) {
    w.u16(p.entity.idx);
    w.u16(p.entity.gen);
    w.u32(p.inv.owned_bits());
    w.u8(p.inv.equipped[0] as u8);
    w.u8(p.inv.equipped[1] as u8);
    w.u8(p.inv.bombs);
    w.u8(p.inv.max_bombs);
    w.u8(p.inv.arrows);
    w.u8(p.inv.max_arrows);
    w.u8(p.inv.keys);
    w.u16(p.inv.rupees);
    w.u8(p.inv.sword_level);
    w.u8(p.inv.heart_pieces);
    w.u16(p.input.buttons);
    w.u16(p.input.prev);
    w.u8(p.role as u8);
    w.u16(p.level);
    w.i32(p.room.0);
    w.i32(p.room.1);
    w.i32(p.camera.x);
    w.i32(p.camera.y);
    w.i32(p.camera.target_x);
    w.i32(p.camera.target_y);
    w.u16(p.camera.scroll);
    w.u8(p.camera.shake);
    w.u16(p.carrying.idx);
    w.u16(p.carrying.gen);
    w.u16(p.respawn);
    w.bool(p.active);
    w.u32(p.deaths);
    w.u32(p.kills);
}

fn read_player(r: &mut Reader) -> Option<Player> {
    let entity = EntityId {
        idx: r.u16()?,
        gen: r.u16()?,
    };
    let mut inv = Inventory::default();
    inv.set_owned_bits(r.u32()?);
    inv.equipped[0] = Item::from_u8(r.u8()?);
    inv.equipped[1] = Item::from_u8(r.u8()?);
    inv.bombs = r.u8()?;
    inv.max_bombs = r.u8()?;
    inv.arrows = r.u8()?;
    inv.max_arrows = r.u8()?;
    inv.keys = r.u8()?;
    inv.rupees = r.u16()?;
    inv.sword_level = r.u8()?;
    inv.heart_pieces = r.u8()?;

    let mut input = Input::default();
    input.buttons = r.u16()?;
    input.prev = r.u16()?;

    let role = if r.u8()? == 1 { Role::Boss } else { Role::Hero };
    let level = r.u16()?;
    let room = (r.i32()?, r.i32()?);
    let camera = Camera {
        x: r.i32()?,
        y: r.i32()?,
        target_x: r.i32()?,
        target_y: r.i32()?,
        scroll: r.u16()?,
        shake: r.u8()?,
    };
    let carrying = EntityId {
        idx: r.u16()?,
        gen: r.u16()?,
    };
    Some(Player {
        entity,
        inv,
        input,
        role,
        level,
        room,
        camera,
        carrying,
        respawn: r.u16()?,
        active: r.bool()?,
        deaths: r.u32()?,
        kills: r.u32()?,
        // Messages are a moment of presentation, not state worth restoring.
        message: None,
    })
}

impl World {
    /// Serialises the entire world.
    pub fn save(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u8(MAGIC[0]);
        w.u8(MAGIC[1]);
        w.u8(MAGIC[2]);
        w.u8(MAGIC[3]);
        w.u16(VERSION);
        w.u64(self.seed);
        w.u64(self.frame);
        w.u64(self.rng.state());

        w.u32(self.levels.len() as u32);
        for l in &self.levels {
            write_level(&mut w, l);
        }

        let slots = self.entities.slots();
        w.u32(slots.len() as u32);
        for e in slots {
            write_entity(&mut w, e);
        }
        // The free list's order decides which slot the next spawn takes, which
        // in turn decides the order entities are stepped in.
        let free = self.entities.free_slots();
        w.u32(free.len() as u32);
        for idx in free {
            w.u16(*idx);
        }

        w.u32(self.players.len() as u32);
        for p in &self.players {
            write_player(&mut w, p);
        }

        w.u32(self.spawned.len() as u32);
        for (level, room) in &self.spawned {
            w.u16(*level);
            w.u16(*room);
        }
        w.finish()
    }

    /// Rebuilds a world from [`World::save`]. Returns `None` for anything that
    /// is not a snapshot this build understands.
    pub fn load(data: &[u8]) -> Option<World> {
        let mut r = Reader::new(data);
        if [r.u8()?, r.u8()?, r.u8()?, r.u8()?] != *MAGIC {
            return None;
        }
        if r.u16()? != VERSION {
            return None;
        }
        let seed = r.u64()?;
        let frame = r.u64()?;
        let rng = Rng::from_state(r.u64()?);

        let level_count = r.u32()? as usize;
        if level_count == 0 || level_count > 256 {
            return None;
        }
        let mut levels = Vec::with_capacity(level_count);
        for _ in 0..level_count {
            levels.push(read_level(&mut r)?);
        }

        let slot_count = r.u32()? as usize;
        if slot_count > 1 << 16 {
            return None;
        }
        let mut slots = Vec::with_capacity(slot_count);
        for _ in 0..slot_count {
            slots.push(read_entity(&mut r)?);
        }
        let free_count = r.u32()? as usize;
        if free_count > slot_count {
            return None;
        }
        let mut free = Vec::with_capacity(free_count);
        for _ in 0..free_count {
            free.push(r.u16()?);
        }

        let player_count = r.u32()? as usize;
        if player_count > 64 {
            return None;
        }
        let mut players = Vec::with_capacity(player_count);
        for _ in 0..player_count {
            players.push(read_player(&mut r)?);
        }

        let spawned_count = r.u32()? as usize;
        let mut spawned = Vec::with_capacity(spawned_count.min(65536));
        for _ in 0..spawned_count {
            spawned.push((r.u16()?, r.u16()?));
        }

        Some(World {
            seed,
            frame,
            rng,
            levels,
            entities: Entities::rebuild(slots, free),
            players,
            events: Events::new(),
            spawned,
        })
    }

    /// A hash of everything that affects play.
    ///
    /// Two machines running the same inputs must agree on this. When they stop
    /// agreeing, the simulations have diverged and the game is no longer the
    /// same game on both screens.
    pub fn checksum(&self) -> u64 {
        // FNV-1a, which is cheap and mixes well enough to catch a stray pixel
        // of divergence.
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mix = |v: u64, h: &mut u64| {
            *h ^= v;
            *h = h.wrapping_mul(0x1000_0000_01b3);
        };
        mix(self.frame, &mut h);
        mix(self.rng.state(), &mut h);
        for l in &self.levels {
            for chunk in l.map.raw().chunks(8) {
                let mut v = 0u64;
                for (i, b) in chunk.iter().enumerate() {
                    v |= (*b as u64) << (i * 8);
                }
                mix(v, &mut h);
            }
        }
        for (id, e) in self.entities.iter() {
            mix(id.idx as u64 | (id.gen as u64) << 16, &mut h);
            mix(e.kind as u64, &mut h);
            mix(e.pos.x as u32 as u64 | (e.pos.y as u32 as u64) << 32, &mut h);
            mix(e.hp as u64 as u64 & 0xffff, &mut h);
            mix(e.state as u64 | (e.timer as u64) << 8, &mut h);
        }
        for p in &self.players {
            mix(p.inv.owned_bits() as u64, &mut h);
            mix(p.inv.rupees as u64 | (p.inv.keys as u64) << 16, &mut h);
            mix(p.kills as u64 | (p.deaths as u64) << 32, &mut h);
            mix(p.respawn as u64, &mut h);
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::button;
    use crate::tiles::tile;

    fn played_world() -> World {
        let mut lv = Level::new(LevelKind::Overworld, 3, 3, tile::GRASS);
        lv.entrance = V2::from_px(80, 64);
        lv.spawns.push(Spawn {
            room: 0,
            kind: Kind::Octorok as u8,
            tx: 3,
            ty: 3,
            param: 0,
        });
        let mut w = World::new(0xfeed_face, vec![lv], 2);
        w.join(0);
        w.join(1);
        for f in 0..200u16 {
            w.set_input(0, if f % 40 < 20 { button::RIGHT } else { button::A });
            w.set_input(1, button::DOWN);
            w.step();
        }
        w
    }

    #[test]
    fn a_snapshot_restores_an_identical_world() {
        let a = played_world();
        let data = a.save();
        let b = World::load(&data).expect("should load");
        assert_eq!(a.checksum(), b.checksum());
        assert_eq!(a.frame, b.frame);
        assert_eq!(a.entities.len(), b.entities.len());
        assert_eq!(
            a.player_entity(0).unwrap().pos,
            b.player_entity(0).unwrap().pos
        );
    }

    #[test]
    fn a_restored_world_keeps_simulating_identically() {
        let mut a = played_world();
        let mut b = World::load(&a.save()).unwrap();
        for f in 0..300u16 {
            let buttons = (f * 13) & 0x3f;
            a.set_input(0, buttons);
            b.set_input(0, buttons);
            a.set_input(1, button::LEFT);
            b.set_input(1, button::LEFT);
            a.step();
            b.step();
        }
        assert_eq!(
            a.checksum(),
            b.checksum(),
            "a world restored from a snapshot must stay in step"
        );
    }

    #[test]
    fn entity_handles_survive_a_round_trip() {
        let mut a = played_world();
        let id = a.spawn(Kind::Chest, 0, V2::from_px(100, 100));
        a.entities.get_mut(id).unwrap().data[0] = 5;
        let b = World::load(&a.save()).unwrap();
        let kept = b.entities.get(id).expect("the handle should still resolve");
        assert_eq!(kept.kind, Kind::Chest);
        assert_eq!(kept.data[0], 5);
    }

    #[test]
    fn the_checksum_notices_a_difference() {
        let a = played_world();
        let mut b = World::load(&a.save()).unwrap();
        assert_eq!(a.checksum(), b.checksum());
        b.entities
            .at_mut(1)
            .map(|e| e.pos = e.pos.add(V2::from_px(1, 0)));
        assert_ne!(a.checksum(), b.checksum());
    }

    #[test]
    fn map_changes_are_part_of_the_snapshot() {
        let mut a = played_world();
        a.levels[0].map.set(4, 4, tile::WALL_CRACKED);
        let b = World::load(&a.save()).unwrap();
        assert_eq!(b.levels[0].map.get(4, 4), tile::WALL_CRACKED);
        assert_eq!(a.checksum(), b.checksum());
    }

    #[test]
    fn inventories_come_back_intact() {
        let mut a = played_world();
        a.players[0].inv.give(Item::Bombs);
        a.players[0].inv.add_bombs(7);
        a.players[0].inv.keys = 3;
        a.players[0].inv.add_rupees(42);
        let b = World::load(&a.save()).unwrap();
        assert!(b.players[0].inv.has(Item::Bombs));
        assert_eq!(b.players[0].inv.bombs, 7);
        assert_eq!(b.players[0].inv.keys, 3);
        assert_eq!(b.players[0].inv.rupees, 42);
    }

    #[test]
    fn rubbish_is_rejected_rather_than_trusted() {
        assert!(World::load(&[]).is_none());
        assert!(World::load(b"not a snapshot at all").is_none());
        // A truncated but well-headed snapshot must not panic.
        let good = played_world().save();
        for cut in [8usize, 32, 100, good.len() / 2, good.len() - 1] {
            assert!(World::load(&good[..cut]).is_none(), "cut at {cut}");
        }
    }

    #[test]
    fn a_snapshot_is_a_sensible_size() {
        let w = played_world();
        let data = w.save();
        // Three screens square of overworld, so a few kilobytes.
        assert!(data.len() < 64 * 1024, "{} bytes", data.len());
        assert!(data.len() > 500);
    }
}
