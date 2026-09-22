//! The simulation.
//!
//! [`World::step`] advances everything by exactly one frame using nothing but
//! the per-player [`Input`] set for that frame. No clocks, no floats, no
//! randomness that is not seeded: replaying the same inputs against the same
//! seed reproduces the world exactly, which is what lockstep multiplayer needs.

use crate::entity::{eflag, info, Entities, Entity, EntityId, Kind};
use crate::event::{Event, Events, Sfx};
use crate::fixed::{px, to_px, Fx, ONE};
use crate::geom::{Dir, Rect, V2};
use crate::input::Input;
use crate::items::{Inventory, Item};
use crate::level::{Level, Spawn, HUD_H, ROOM_PX_H, ROOM_PX_W, SCREEN_H, SCREEN_W, TILE_PX};
use crate::rng::Rng;
use crate::tiles::{self, flag, tile};

/// Frames a player is immune after taking a hit.
pub const PLAYER_IFRAMES: u8 = 48;
/// Frames the camera takes to scroll between rooms.
pub const ROOM_SCROLL_FRAMES: u16 = 14;
/// Frames between dying and respawning.
pub const RESPAWN_FRAMES: u16 = 90;
/// Height of the visible playfield, below the status bar.
pub const VIEW_H: i32 = SCREEN_H - HUD_H;
/// Width of the visible playfield.
pub const VIEW_W: i32 = SCREEN_W;

/// What a player is currently doing.
pub mod pstate {
    pub const NORMAL: u8 = 0;
    pub const ATTACK: u8 = 1;
    pub const HURT: u8 = 2;
    pub const LIFT: u8 = 3;
    pub const THROW: u8 = 4;
    pub const JUMP: u8 = 5;
    pub const SWIM: u8 = 6;
    pub const FALL: u8 = 7;
    pub const DEAD: u8 = 8;
    pub const WARP: u8 = 9;
}

/// Which side a player is playing on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum Role {
    /// An ordinary hero.
    #[default]
    Hero = 0,
    /// A player who has taken control of a boss.
    Boss = 1,
}

/// A per-player view of the world.
#[derive(Clone, Copy, Debug, Default)]
pub struct Camera {
    /// Top-left of the viewport in world pixels, fixed point.
    pub x: Fx,
    pub y: Fx,
    /// Where the camera is heading.
    pub target_x: Fx,
    pub target_y: Fx,
    /// Frames of scroll remaining.
    pub scroll: u16,
    /// Frames of screen shake remaining.
    pub shake: u8,
}

impl Camera {
    fn snap(&mut self, x: Fx, y: Fx) {
        self.x = x;
        self.y = y;
        self.target_x = x;
        self.target_y = y;
        self.scroll = 0;
    }

    fn step(&mut self) {
        if self.scroll > 0 {
            let n = self.scroll as Fx;
            self.x += (self.target_x - self.x) / n;
            self.y += (self.target_y - self.y) / n;
            self.scroll -= 1;
            if self.scroll == 0 {
                self.x = self.target_x;
                self.y = self.target_y;
            }
        }
        self.shake = self.shake.saturating_sub(1);
    }
}

/// One participant, local or remote.
#[derive(Clone, Debug)]
pub struct Player {
    /// The entity this player is driving. For a hero that is their Link; for a
    /// boss player it is the boss entity.
    pub entity: EntityId,
    pub inv: Inventory,
    pub input: Input,
    pub role: Role,
    pub level: u16,
    /// Room coordinates the camera is showing.
    pub room: (i32, i32),
    pub camera: Camera,
    /// The object this player has picked up, if any.
    pub carrying: EntityId,
    /// Frames until this player respawns, 0 when alive.
    pub respawn: u16,
    /// True once someone has joined this slot.
    pub active: bool,
    pub deaths: u32,
    pub kills: u32,
    /// A message shown in the status bar, with frames remaining.
    pub message: Option<(&'static str, u16)>,
}

impl Default for Player {
    fn default() -> Self {
        Player {
            entity: EntityId::NONE,
            inv: Inventory::default(),
            input: Input::default(),
            role: Role::Hero,
            level: 0,
            room: (0, 0),
            camera: Camera::default(),
            carrying: EntityId::NONE,
            respawn: 0,
            active: false,
            deaths: 0,
            kills: 0,
            message: None,
        }
    }
}

impl Player {
    /// True when the player is in the world and not waiting to respawn.
    pub fn is_alive(&self) -> bool {
        self.active && self.respawn == 0 && !self.entity.is_none()
    }
}

/// The whole game state.
pub struct World {
    pub seed: u64,
    /// Frames simulated so far. Part of the RNG stream, so it must stay in sync.
    pub frame: u64,
    pub rng: Rng,
    pub levels: Vec<Level>,
    pub entities: Entities,
    pub players: Vec<Player>,
    pub events: Events,
    /// Rooms whose contents have been spawned, keyed by (level, room index).
    spawned: Vec<(u16, u16)>,
}

impl World {
    /// Creates a world from generated levels.
    pub fn new(seed: u64, levels: Vec<Level>, player_count: usize) -> World {
        let mut w = World {
            seed,
            frame: 0,
            rng: Rng::new(seed ^ 0xb0b1_c0de),
            levels,
            entities: Entities::new(),
            players: vec![Player::default(); player_count.max(1)],
            events: Events::new(),
            spawned: Vec::new(),
        };
        if w.levels.is_empty() {
            w.levels.push(Level::new(
                crate::level::LevelKind::Overworld,
                1,
                1,
                tile::GRASS,
            ));
        }
        w
    }

    /// Brings a player into the world at the first level's entrance.
    pub fn join(&mut self, player: usize) -> bool {
        if player >= self.players.len() {
            return false;
        }
        if self.players[player].active {
            return true;
        }
        let entrance = self.levels[0].entrance;
        let id = self.entities.spawn(Kind::Player, 0, entrance);
        if let Some(e) = self.entities.get_mut(id) {
            e.player = player as u8;
            e.dir = Dir::Down;
            // Where to put the player back after a fall or a dunking.
            e.data[0] = entrance.x;
            e.data[1] = entrance.y;
        }
        let room = self.levels[0].room_at(entrance);
        let p = &mut self.players[player];
        *p = Player {
            entity: id,
            active: true,
            level: 0,
            room,
            ..Player::default()
        };
        let (cx, cy) = camera_for_room(&self.levels[0], room);
        p.camera.snap(cx, cy);
        true
    }

    /// Removes a player from the world, e.g. on disconnect.
    pub fn leave(&mut self, player: usize) {
        if let Some(p) = self.players.get(player).cloned() {
            if !p.carrying.is_none() {
                self.entities.despawn(p.carrying);
            }
            if !p.entity.is_none() {
                self.entities.despawn(p.entity);
            }
        }
        if let Some(p) = self.players.get_mut(player) {
            *p = Player::default();
        }
    }

    /// Sets the buttons held by a player for the coming frame.
    pub fn set_input(&mut self, player: usize, buttons: u16) {
        if let Some(p) = self.players.get_mut(player) {
            p.input.advance(buttons);
        }
    }

    /// The entity a player is driving.
    pub fn player_entity(&self, player: usize) -> Option<&Entity> {
        self.players
            .get(player)
            .and_then(|p| self.entities.get(p.entity))
    }

    /// The level a player is on.
    pub fn player_level(&self, player: usize) -> &Level {
        let idx = self
            .players
            .get(player)
            .map(|p| p.level as usize)
            .unwrap_or(0);
        &self.levels[idx.min(self.levels.len() - 1)]
    }

    /// Advances the simulation by one frame.
    pub fn step(&mut self) {
        self.events.clear();
        self.frame = self.frame.wrapping_add(1);

        self.activate_rooms();

        for i in 0..self.players.len() {
            if self.players[i].active {
                crate::player::update(self, i);
            }
        }

        for idx in 0..self.entities.capacity() {
            let Some(e) = self.entities.at(idx) else {
                continue;
            };
            if e.kind == Kind::Player || !e.alive {
                continue;
            }
            if e.kind.is_enemy() {
                if e.has(eflag::POSSESSED) {
                    // A player is driving this monster; their input already did.
                    continue;
                }
                crate::enemy::update(self, idx);
            } else {
                crate::objects::update(self, idx);
            }
        }

        crate::combat::resolve(self);
        self.separate_bodies();
        self.cull_far_entities();

        for i in 0..self.players.len() {
            let p = &mut self.players[i];
            p.camera.step();
            if let Some((_, t)) = &mut p.message {
                *t = t.saturating_sub(1);
                if *t == 0 {
                    p.message = None;
                }
            }
            if p.respawn > 0 {
                p.respawn -= 1;
                if p.respawn == 0 {
                    self.respawn_player(i);
                }
            }
        }

        self.entities.collect_dead();
    }

    /// A per-frame random stream that is independent of draw order.
    pub fn rand(&mut self) -> &mut Rng {
        &mut self.rng
    }

    // ----- world queries -------------------------------------------------

    pub fn level(&self, idx: u16) -> &Level {
        &self.levels[(idx as usize).min(self.levels.len() - 1)]
    }

    pub fn level_mut(&mut self, idx: u16) -> &mut Level {
        let n = self.levels.len() - 1;
        &mut self.levels[(idx as usize).min(n)]
    }

    /// Finds the closest living player entity on a level, within `max_px`.
    pub fn nearest_player(&self, level: u16, from: V2, max_px: i32) -> Option<(EntityId, V2)> {
        let mut best: Option<(EntityId, V2, i64)> = None;
        for (id, e) in self.entities.iter() {
            if e.kind != Kind::Player || e.level != level {
                continue;
            }
            let p = &self.players[e.player as usize % self.players.len().max(1)];
            if !p.is_alive() {
                continue;
            }
            let dx = (e.pos.x - from.x) as i64;
            let dy = (e.pos.y - from.y) as i64;
            let d2 = dx * dx + dy * dy;
            if d2 <= (px(max_px) as i64).pow(2) && best.map(|b| d2 < b.2).unwrap_or(true) {
                best = Some((id, e.pos, d2));
            }
        }
        best.map(|(id, p, _)| (id, p))
    }

    /// True when any player is within `rooms` rooms of a position.
    pub fn is_near_player(&self, level: u16, pos: V2, rooms: i32) -> bool {
        let (rx, ry) = self.level(level).room_at(pos);
        self.players.iter().any(|p| {
            p.active
                && p.level == level
                && (p.room.0 - rx).abs() <= rooms
                && (p.room.1 - ry).abs() <= rooms
        })
    }

    // ----- mutation helpers ----------------------------------------------

    /// Spawns an entity and returns its handle.
    pub fn spawn(&mut self, kind: Kind, level: u16, pos: V2) -> EntityId {
        self.entities.spawn(kind, level, pos)
    }

    /// Applies damage to an entity, with knockback away from `from`.
    ///
    /// Returns true when the hit landed.
    pub fn damage(&mut self, target: EntityId, amount: i16, from: V2, knock_px: i32) -> bool {
        let Some(e) = self.entities.get(target) else {
            return false;
        };
        if !e.can_be_hurt() {
            return false;
        }
        let (pos, is_boss, is_player, player_idx) = (
            e.pos,
            e.has(eflag::BOSS),
            e.kind == Kind::Player,
            e.player,
        );
        if is_player {
            return self.hurt_player(player_idx as usize, amount, from);
        }
        let e = self.entities.get_mut(target).unwrap();
        e.hp -= amount;
        e.iframes = 16;
        e.stun = 8;
        if knock_px > 0 && !is_boss {
            let dir = pos.sub(from).with_length(px(knock_px));
            e.knock = dir;
            e.knock_frames = 6;
        }
        let dead = e.hp <= 0;
        self.events.push(Event::Hit {
            at: pos,
            heavy: is_boss,
        });
        self.events.sound(if is_boss {
            Sfx::BossHurt
        } else {
            Sfx::EnemyHit
        });
        if dead {
            self.kill(target);
        }
        true
    }

    /// Kills an entity, dropping loot and emitting effects.
    pub fn kill(&mut self, target: EntityId) {
        let Some(e) = self.entities.get(target) else {
            return;
        };
        let (kind, pos, level, is_boss) = (e.kind, e.pos, e.level, e.has(eflag::BOSS));
        self.entities.despawn(target);
        let poof = self.spawn(Kind::Poof, level, pos);
        if let Some(p) = self.entities.get_mut(poof) {
            p.timer = 18;
        }
        if kind.is_enemy() {
            self.events.sound(if is_boss { Sfx::BossDie } else { Sfx::EnemyDie });
            if is_boss {
                self.events.push(Event::BossDefeated { level });
                self.events.push(Event::Shake { frames: 30 });
                self.on_boss_defeated(level, pos);
            } else {
                self.drop_loot(level, pos);
            }
            // A Zol splits when it dies.
            if kind == Kind::Zol {
                self.split_zol(level, pos);
            }
        }
    }

    fn split_zol(&mut self, level: u16, pos: V2) {
        let small = self.rng.below(2) == 0;
        if !small {
            return;
        }
        for i in 0..2 {
            let off = px(if i == 0 { -6 } else { 6 });
            let id = self.spawn(Kind::Zol, level, V2::new(pos.x + off, pos.y));
            if let Some(e) = self.entities.get_mut(id) {
                e.hp = 1;
                e.max_hp = 1;
                e.body_w = 8;
                e.body_h = 8;
                e.data[0] = 1;
            }
        }
    }

    fn drop_loot(&mut self, level: u16, pos: V2) {
        let roll = self.rng.below(100);
        let kind = match roll {
            0..=24 => Kind::Heart,
            25..=54 => Kind::Rupee,
            55..=62 => Kind::BombPickup,
            63..=68 => Kind::ArrowPickup,
            _ => return,
        };
        let id = self.spawn(kind, level, pos);
        if let Some(e) = self.entities.get_mut(id) {
            // Pickups vanish after a while so rooms do not fill up.
            e.timer = 480;
        }
    }

    /// Opens the boss room and drops the prize when a boss dies.
    fn on_boss_defeated(&mut self, level: u16, pos: V2) {
        let lv = self.level_mut(level);
        for t in 0..lv.map.w() * lv.map.h() {
            let (tx, ty) = (t % lv.map.w(), t / lv.map.w());
            if lv.map.get(tx, ty) == tile::DOOR_BOSS || lv.map.get(tx, ty) == tile::DOOR_SHUT {
                lv.map.set(tx, ty, tile::DOOR_OPEN);
            }
        }
        let id = self.spawn(Kind::HeartPiece, level, pos);
        if let Some(e) = self.entities.get_mut(id) {
            e.timer = 0;
        }
    }

    /// Damages a player, returning true when the hit landed.
    pub fn hurt_player(&mut self, player: usize, amount: i16, from: V2) -> bool {
        self.hurt_player_with(player, amount, from, 3)
    }

    /// Damages a player with an explicit knockback distance.
    ///
    /// Hazards that put the player somewhere safe first (pits, water) pass
    /// zero, otherwise the knockback would shove them straight back in.
    pub fn hurt_player_with(
        &mut self,
        player: usize,
        amount: i16,
        from: V2,
        knock_px: i32,
    ) -> bool {
        let Some(p) = self.players.get(player) else {
            return false;
        };
        if !p.is_alive() {
            return false;
        }
        let eid = p.entity;
        let Some(e) = self.entities.get(eid) else {
            return false;
        };
        if e.iframes > 0 || e.hp <= 0 {
            return false;
        }
        let pos = e.pos;
        let e = self.entities.get_mut(eid).unwrap();
        e.hp -= amount;
        e.iframes = PLAYER_IFRAMES;
        e.state = pstate::HURT;
        e.timer = 14;
        if knock_px > 0 {
            e.knock = pos.sub(from).with_length(px(knock_px));
            e.knock_frames = 10;
        } else {
            e.knock = V2::ZERO;
            e.knock_frames = 0;
        }
        let died = e.hp <= 0;
        self.events.push(Event::PlayerHurt {
            player: player as u8,
            amount,
        });
        self.events.sound(Sfx::PlayerHurt);
        self.events.push(Event::Shake { frames: 6 });
        if died {
            self.kill_player(player);
        }
        true
    }

    /// Kills a player and starts their respawn timer.
    pub fn kill_player(&mut self, player: usize) {
        let Some(p) = self.players.get(player).cloned() else {
            return;
        };
        if !p.carrying.is_none() {
            self.entities.despawn(p.carrying);
        }
        if let Some(e) = self.entities.get_mut(p.entity) {
            e.hp = 0;
            e.state = pstate::DEAD;
            e.vel = V2::ZERO;
            e.timer = RESPAWN_FRAMES;
            // Knockback from the killing blow must not carry over past the
            // respawn, or the player slides away from the entrance.
            e.knock = V2::ZERO;
            e.knock_frames = 0;
        }
        let p = &mut self.players[player];
        p.carrying = EntityId::NONE;
        p.respawn = RESPAWN_FRAMES;
        p.deaths += 1;
        self.events.push(Event::PlayerDied {
            player: player as u8,
        });
        self.events.sound(Sfx::PlayerDie);
    }

    fn respawn_player(&mut self, player: usize) {
        let entrance = self.levels[0].entrance;
        let eid = self.players[player].entity;
        let max_hp = info(Kind::Player).hp;
        if let Some(e) = self.entities.get_mut(eid) {
            e.hp = max_hp;
            e.max_hp = max_hp;
            e.state = pstate::NORMAL;
            e.timer = 0;
            e.level = 0;
            e.pos = entrance;
            e.vel = V2::ZERO;
            e.z = 0;
            e.vz = 0;
            e.iframes = PLAYER_IFRAMES;
            e.knock = V2::ZERO;
            e.knock_frames = 0;
            e.data[0] = entrance.x;
            e.data[1] = entrance.y;
        } else {
            let id = self.entities.spawn(Kind::Player, 0, entrance);
            if let Some(e) = self.entities.get_mut(id) {
                e.player = player as u8;
            }
            self.players[player].entity = id;
        }
        // Dying costs half of the player's rupees, the classic sting.
        let room = self.levels[0].room_at(entrance);
        let (cx, cy) = camera_for_room(&self.levels[0], room);
        let p = &mut self.players[player];
        p.level = 0;
        p.room = room;
        p.camera.snap(cx, cy);
        p.inv.rupees /= 2;
    }

    /// Moves a player to another level, e.g. through a staircase.
    pub fn warp_player(&mut self, player: usize, to_level: u16, to_pos: V2, dir: Dir) {
        let to_level = to_level.min(self.levels.len() as u16 - 1);
        let eid = self.players[player].entity;
        if let Some(e) = self.entities.get_mut(eid) {
            e.level = to_level;
            e.pos = to_pos;
            e.dir = dir;
            e.vel = V2::ZERO;
            e.state = pstate::NORMAL;
            e.data[0] = to_pos.x;
            e.data[1] = to_pos.y;
        }
        let room = self.level(to_level).room_at(to_pos);
        let (cx, cy) = camera_for_room(self.level(to_level), room);
        let p = &mut self.players[player];
        p.level = to_level;
        p.room = room;
        p.camera.snap(cx, cy);
        self.events.push(Event::LevelChanged {
            player: player as u8,
            level: to_level,
        });
        self.events.sound(Sfx::Stairs);
    }

    /// Shows a short message in a player's status bar.
    pub fn say(&mut self, player: usize, text: &'static str) {
        if let Some(p) = self.players.get_mut(player) {
            p.message = Some((text, 150));
        }
        self.events.push(Event::Message(text));
    }

    // ----- room streaming -------------------------------------------------

    /// Spawns the contents of any room a player has just walked into.
    fn activate_rooms(&mut self) {
        let mut wanted: Vec<(u16, u16)> = Vec::new();
        for p in &self.players {
            if !p.active {
                continue;
            }
            let lv = &self.levels[p.level as usize];
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if let Some(i) = lv.room_index(p.room.0 + dx, p.room.1 + dy) {
                        wanted.push((p.level, i as u16));
                    }
                }
            }
        }
        wanted.sort_unstable();
        wanted.dedup();

        for (level, room) in wanted {
            if self.spawned.contains(&(level, room)) {
                continue;
            }
            self.spawned.push((level, room));
            let spawns: Vec<Spawn> = self.levels[level as usize]
                .spawns
                .iter()
                .filter(|s| s.room == room)
                .copied()
                .collect();
            for s in spawns {
                let kind = Kind::from_u8(s.kind);
                if kind == Kind::None {
                    continue;
                }
                let pos = crate::level::tile_center(s.tx, s.ty);
                let id = self.spawn(kind, level, pos);
                if let Some(e) = self.entities.get_mut(id) {
                    e.data[0] = s.param;
                    if kind == Kind::Chest {
                        e.state = 0;
                    }
                }
            }
            if let Some(r) = self.levels[level as usize].rooms.get_mut(room as usize) {
                r.visited = true;
            }
        }
    }

    /// Removes entities that have wandered far from every player, and lets
    /// their room repopulate when someone comes back.
    fn cull_far_entities(&mut self) {
        let mut freed: Vec<(u16, u16)> = Vec::new();
        for idx in 0..self.entities.capacity() {
            let Some(e) = self.entities.at(idx) else {
                continue;
            };
            if !e.has(eflag::DESPAWN_FAR) || e.has(eflag::POSSESSED) {
                continue;
            }
            let (level, pos) = (e.level, e.pos);
            if !self.is_near_player(level, pos, 2) {
                let lv = &self.levels[level as usize];
                let (rx, ry) = lv.room_at(pos);
                if let Some(ri) = lv.room_index(rx, ry) {
                    freed.push((level, ri as u16));
                }
                let id = self.entities.id_of(idx);
                self.entities.despawn(id);
            }
        }
        for key in freed {
            // Allow the room to spawn a fresh set next time it is entered.
            if !self.room_has_entities(key.0, key.1) {
                self.spawned.retain(|k| *k != key);
            }
        }
    }

    fn room_has_entities(&self, level: u16, room: u16) -> bool {
        let lv = &self.levels[level as usize];
        self.entities.iter().any(|(_, e)| {
            if e.level != level || !e.has(eflag::DESPAWN_FAR) {
                return false;
            }
            let (rx, ry) = lv.room_at(e.pos);
            lv.room_index(rx, ry) == Some(room as usize)
        })
    }

    /// Pushes overlapping solid bodies apart so monsters do not stack.
    fn separate_bodies(&mut self) {
        let solids: Vec<(usize, u16, Rect)> = (0..self.entities.capacity())
            .filter_map(|i| {
                let e = self.entities.at(i)?;
                if e.has(eflag::SOLID_BODY) && !e.has(eflag::BOSS) {
                    Some((i, e.level, e.body()))
                } else {
                    None
                }
            })
            .collect();
        for a in 0..solids.len() {
            for b in (a + 1)..solids.len() {
                if solids[a].1 != solids[b].1 {
                    continue;
                }
                let push = crate::physics::separate(&solids[a].2, &solids[b].2, ONE / 2);
                if push.x == 0 && push.y == 0 {
                    continue;
                }
                if let Some(e) = self.entities.at_mut(solids[a].0) {
                    e.pos = e.pos.add(push);
                }
                if let Some(e) = self.entities.at_mut(solids[b].0) {
                    e.pos = e.pos.sub(push);
                }
            }
        }
    }

    /// Replaces a tile and plays the effects for breaking it.
    pub fn break_tile(&mut self, level: u16, tx: i32, ty: i32) {
        let t = self.level(level).map.get(tx, ty);
        let next = tiles::destroyed(t);
        if next == t {
            return;
        }
        self.level_mut(level).map.set(tx, ty, next);
        let pos = crate::level::tile_center(tx, ty);
        let id = self.spawn(Kind::Poof, level, pos);
        if let Some(e) = self.entities.get_mut(id) {
            e.timer = 16;
        }
        if tiles::any(t, flag::BOMBABLE) {
            self.events.sound(Sfx::Secret);
        }
    }

    /// Applies an explosion: hurts everyone nearby and breaks weak walls.
    pub fn explode(&mut self, level: u16, pos: V2) {
        let id = self.spawn(Kind::Explosion, level, pos);
        if let Some(e) = self.entities.get_mut(id) {
            e.timer = 24;
        }
        self.events.sound(Sfx::Explosion);
        self.events.push(Event::Shake { frames: 12 });

        let blast = Rect::centered(pos, 34, 34);
        let targets: Vec<EntityId> = self
            .entities
            .iter()
            .filter(|(_, e)| e.level == level && e.hurt_box().overlaps(&blast))
            .map(|(id, _)| id)
            .collect();
        for t in targets {
            let Some(e) = self.entities.get(t) else {
                continue;
            };
            if e.kind == Kind::Player {
                let pi = e.player as usize;
                self.hurt_player(pi, 2, pos);
            } else if e.has(eflag::VULNERABLE) {
                self.damage(t, 4, pos, 6);
            }
        }

        let (tx, ty) = crate::physics::tile_coords(pos);
        for dy in -1..=1 {
            for dx in -1..=1 {
                let t = self.level(level).map.get(tx + dx, ty + dy);
                if tiles::any(t, flag::BOMBABLE | flag::CUTTABLE | flag::LIFTABLE) {
                    self.break_tile(level, tx + dx, ty + dy);
                }
            }
        }
    }

    /// Gives a pickup to a player.
    pub fn collect(&mut self, player: usize, kind: Kind) {
        let p = &mut self.players[player];
        match kind {
            Kind::Rupee => {
                p.inv.add_rupees(1);
                self.events.sound(Sfx::Rupee);
            }
            Kind::Heart => {
                let eid = p.entity;
                if let Some(e) = self.entities.get_mut(eid) {
                    e.hp = (e.hp + 4).min(e.max_hp);
                }
                self.events.sound(Sfx::Heart);
            }
            Kind::Key => {
                p.inv.keys = p.inv.keys.saturating_add(1);
                self.events.sound(Sfx::Pickup);
            }
            Kind::BombPickup => {
                p.inv.give(Item::Bombs);
                p.inv.add_bombs(3);
                self.events.sound(Sfx::Pickup);
            }
            Kind::ArrowPickup => {
                p.inv.give(Item::Bow);
                p.inv.add_arrows(5);
                self.events.sound(Sfx::Pickup);
            }
            Kind::Fairy => {
                let eid = p.entity;
                if let Some(e) = self.entities.get_mut(eid) {
                    e.hp = e.max_hp;
                }
                self.events.sound(Sfx::Secret);
            }
            Kind::HeartPiece => {
                let eid = p.entity;
                p.inv.heart_pieces = p.inv.heart_pieces.saturating_add(1);
                if let Some(e) = self.entities.get_mut(eid) {
                    e.max_hp = (e.max_hp + 4).min(4 * 14);
                    e.hp = e.max_hp;
                }
                self.events.sound(Sfx::Secret);
                self.say(player, "HEART CONTAINER!");
            }
            Kind::Triforce => {
                self.events.sound(Sfx::Secret);
                self.say(player, "YOU WIN!");
            }
            _ => {}
        }
    }
}

/// Camera position that frames a room, clamped to the level.
pub fn camera_for_room(level: &Level, room: (i32, i32)) -> (Fx, Fx) {
    let max_x = (level.map.w() * TILE_PX - VIEW_W).max(0);
    let max_y = (level.map.h() * TILE_PX - VIEW_H).max(0);
    let x = (room.0 * ROOM_PX_W).clamp(0, max_x);
    let y = (room.1 * ROOM_PX_H).clamp(0, max_y);
    (px(x), px(y))
}

/// Converts a world position to screen pixels for a given camera.
pub fn to_screen(cam: &Camera, p: V2) -> (i32, i32) {
    (to_px(p.x - cam.x), to_px(p.y - cam.y) + HUD_H)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::LevelKind;

    fn test_world() -> World {
        let mut lv = Level::new(LevelKind::Overworld, 2, 2, tile::GRASS);
        lv.entrance = V2::from_px(80, 64);
        let mut w = World::new(1234, vec![lv], 2);
        w.join(0);
        w
    }

    #[test]
    fn joining_places_a_player() {
        let w = test_world();
        assert!(w.players[0].is_alive());
        let e = w.player_entity(0).unwrap();
        assert_eq!(e.kind, Kind::Player);
        assert_eq!(e.pos, V2::from_px(80, 64));
    }

    #[test]
    fn stepping_is_deterministic() {
        let mut a = test_world();
        let mut b = test_world();
        for f in 0..300u16 {
            let buttons = (f as u16 * 37) & 0x3f;
            a.set_input(0, buttons);
            b.set_input(0, buttons);
            a.step();
            b.step();
        }
        let ea = a.player_entity(0).unwrap();
        let eb = b.player_entity(0).unwrap();
        assert_eq!(ea.pos, eb.pos);
        assert_eq!(a.rng.state(), b.rng.state());
        assert_eq!(a.entities.len(), b.entities.len());
    }

    #[test]
    fn damage_kills_and_drops() {
        let mut w = test_world();
        let id = w.spawn(Kind::Octorok, 0, V2::from_px(100, 64));
        assert!(w.damage(id, 99, V2::from_px(90, 64), 4));
        assert!(w.entities.get(id).is_none(), "the octorok should be dead");
        assert!(w.events.has_sound(Sfx::EnemyDie));
    }

    #[test]
    fn iframes_block_repeat_damage() {
        let mut w = test_world();
        let id = w.spawn(Kind::Moblin, 0, V2::from_px(100, 64));
        assert!(w.damage(id, 1, V2::from_px(90, 64), 0));
        assert!(!w.damage(id, 1, V2::from_px(90, 64), 0), "still invulnerable");
        assert_eq!(w.entities.get(id).unwrap().hp, 2);
    }

    #[test]
    fn a_dying_player_respawns_at_the_entrance() {
        let mut w = test_world();
        let eid = w.players[0].entity;
        w.entities.get_mut(eid).unwrap().pos = V2::from_px(200, 200);
        w.hurt_player(0, 99, V2::from_px(190, 200));
        assert_eq!(w.players[0].deaths, 1);
        assert!(!w.players[0].is_alive());
        for _ in 0..RESPAWN_FRAMES + 2 {
            w.set_input(0, 0);
            w.step();
        }
        assert!(w.players[0].is_alive());
        assert_eq!(w.player_entity(0).unwrap().pos, V2::from_px(80, 64));
    }

    #[test]
    fn explosions_break_bombable_walls() {
        let mut w = test_world();
        w.levels[0].map.set(6, 4, tile::WALL_CRACKED);
        w.explode(0, crate::level::tile_center(6, 5));
        assert_eq!(w.levels[0].map.get(6, 4), tile::FLOOR);
    }

    #[test]
    fn explosions_hurt_the_player_who_set_them() {
        let mut w = test_world();
        let pos = w.player_entity(0).unwrap().pos;
        w.explode(0, pos);
        assert!(w.player_entity(0).unwrap().hp < 12);
    }

    #[test]
    fn room_spawns_populate_once_a_player_is_near() {
        let mut lv = Level::new(LevelKind::Overworld, 2, 1, tile::GRASS);
        lv.entrance = V2::from_px(80, 64);
        let room = lv.room_index(0, 0).unwrap() as u16;
        lv.spawns.push(Spawn {
            room,
            kind: Kind::Octorok as u8,
            tx: 3,
            ty: 3,
            param: 0,
        });
        let mut w = World::new(9, vec![lv], 1);
        w.join(0);
        w.step();
        assert!(
            w.entities.iter().any(|(_, e)| e.kind == Kind::Octorok),
            "the room's octorok should have spawned"
        );
    }

    #[test]
    fn camera_stays_inside_the_level() {
        let lv = Level::new(LevelKind::Overworld, 1, 1, tile::GRASS);
        let (x, y) = camera_for_room(&lv, (0, 0));
        assert_eq!((x, y), (0, 0));
        let (x, y) = camera_for_room(&lv, (5, 5));
        assert_eq!(to_px(x), 0, "a one-room level cannot scroll");
        assert_eq!(to_px(y), ROOM_PX_H - VIEW_H);
    }

    #[test]
    fn leaving_removes_the_player_entity() {
        let mut w = test_world();
        let id = w.players[0].entity;
        w.leave(0);
        assert!(w.entities.get(id).is_none());
        assert!(!w.players[0].active);
    }
}
