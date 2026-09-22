//! Playing as the monster.
//!
//! A boss player drives a boss entity instead of a hero. Nothing else in the
//! simulation changes: the boss is still an enemy, so heroes' swords still
//! hurt it and its touch still hurts them. Possession only swaps out who
//! decides where it goes, which is why this is a short file rather than a
//! second combat system.

use crate::entity::{eflag, info, Entity, EntityId, Kind};
use crate::event::Sfx;
use crate::fixed::px;
use crate::geom::{Dir, V2};
use crate::input::button;
use crate::level::Spawn;
use crate::world::{camera_for_room, Role, World, ROOM_SCROLL_FRAMES};

/// Frames between a boss player's slams.
const SLAM_COOLDOWN: u16 = 36;
/// Frames between a boss player's fireball rings.
const RING_COOLDOWN: u16 = 90;

impl World {
    /// Hands a player control of a boss.
    ///
    /// Finds a boss already in the world, or wakes one from the level data if
    /// none has been spawned yet. Returns false when the world holds no boss
    /// at all, which is the case for a world generated without dungeons.
    pub fn possess_boss(&mut self, player: usize) -> bool {
        if player >= self.players.len() {
            return false;
        }
        let existing = self
            .entities
            .iter()
            .find(|(_, e)| e.kind == Kind::Boss && !e.has(eflag::POSSESSED))
            .map(|(id, _)| id);

        let boss = match existing {
            Some(id) => id,
            None => match self.spawn_a_boss() {
                Some(id) => id,
                None => return false,
            },
        };

        // Let go of whatever this player was driving.
        let old = self.players[player].entity;
        if old != boss && !old.is_none() {
            self.entities.despawn(old);
        }

        let (level, pos) = {
            let Some(e) = self.entities.get_mut(boss) else {
                return false;
            };
            e.flags |= eflag::POSSESSED;
            e.player = player as u8;
            // A player-driven boss keeps its own room rather than despawning
            // when the heroes wander off.
            e.flags &= !eflag::DESPAWN_FAR;
            (e.level, e.pos)
        };

        let room = self.level(level).room_at(pos);
        let (cx, cy) = camera_for_room(self.level(level), room);
        let p = &mut self.players[player];
        p.entity = boss;
        p.role = Role::Boss;
        p.level = level;
        p.room = room;
        p.active = true;
        p.respawn = 0;
        p.carrying = EntityId::NONE;
        p.camera.x = cx;
        p.camera.y = cy;
        p.camera.target_x = cx;
        p.camera.target_y = cy;
        p.camera.scroll = 0;
        self.say(player, "YOU ARE THE BOSS");
        true
    }

    /// Spawns a boss from a dungeon's own boss placement.
    fn spawn_a_boss(&mut self) -> Option<EntityId> {
        let mut found: Option<(u16, Spawn)> = None;
        for (i, level) in self.levels.iter().enumerate() {
            if let Some(s) = level.spawns.iter().find(|s| s.kind == Kind::Boss as u8) {
                found = Some((i as u16, *s));
                break;
            }
        }
        let (level, spawn) = found?;
        let pos = crate::level::tile_center(spawn.tx, spawn.ty);
        let id = self.spawn(Kind::Boss, level, pos);
        if let Some(e) = self.entities.get_mut(id) {
            e.data[0] = spawn.param;
        }
        // Mark the room as populated so the streamer does not add a second one.
        if let Some(room) = self.level(level).room_index(
            spawn.tx / crate::level::ROOM_W,
            spawn.ty / crate::level::ROOM_H,
        ) {
            let key = (level, room as u16);
            if !self.spawned.contains(&key) {
                self.spawned.push(key);
            }
        }
        Some(id)
    }

    /// Gives up control of a boss and puts the player back to being a hero.
    pub fn become_hero(&mut self, player: usize) {
        if player >= self.players.len() {
            return;
        }
        let old = self.players[player].entity;
        if let Some(e) = self.entities.get_mut(old) {
            if e.kind == Kind::Boss {
                e.flags &= !eflag::POSSESSED;
                e.flags |= eflag::DESPAWN_FAR;
                e.player = u8::MAX;
            }
        }
        self.players[player] = crate::world::Player {
            role: Role::Hero,
            ..crate::world::Player::default()
        };
        self.join(player);
    }
}

/// Advances a player-driven boss for one frame.
pub(crate) fn update(world: &mut World, pi: usize) {
    let eid = world.players[pi].entity;
    let Some(mut e) = world.entities.get(eid).cloned() else {
        // The heroes killed it. Back to being one of them.
        world.become_hero(pi);
        return;
    };

    e.iframes = e.iframes.saturating_sub(1);
    e.stun = e.stun.saturating_sub(1);
    e.timer = e.timer.saturating_sub(1);
    e.anim_timer = e.anim_timer.wrapping_add(1);

    let input = world.players[pi].input;
    let level = e.level;

    if e.knock_frames > 0 {
        e.knock_frames -= 1;
        let k = e.knock;
        crate::physics::move_by(&world.levels[level as usize].map, &mut e, k);
        if e.knock_frames == 0 {
            e.knock = V2::ZERO;
        }
    }

    if e.stun == 0 {
        let (ax, ay) = (input.axis_x(), input.axis_y());
        if ax != 0 || ay != 0 {
            e.dir = Dir::from_delta(px(ax), px(ay));
            let speed = info(Kind::Boss).speed;
            let mut delta = V2::new(speed * ax, speed * ay);
            if ax != 0 && ay != 0 {
                delta = V2::new(delta.x * 181 / 256, delta.y * 181 / 256);
            }
            crate::physics::move_by(&world.levels[level as usize].map, &mut e, delta);
        }

        // A slam: a short, heavy blow to everything standing close by.
        if input.pressed(button::A) && e.data[1] <= 0 {
            e.data[1] = SLAM_COOLDOWN as i32;
            let at = e.pos;
            write_back(world, eid, e.clone());
            slam(world, pi, at);
            if let Some(updated) = world.entities.get(eid).cloned() {
                e = updated;
            }
        }
        // A ring of fire, on a longer leash.
        if input.pressed(button::B) && e.data[2] <= 0 {
            e.data[2] = RING_COOLDOWN as i32;
            write_back(world, eid, e.clone());
            ring_of_fire(world, eid);
            if let Some(updated) = world.entities.get(eid).cloned() {
                e = updated;
            }
        }
    }

    e.data[1] = (e.data[1] - 1).max(0);
    e.data[2] = (e.data[2] - 1).max(0);

    let level_ref = &world.levels[level as usize];
    e.pos = level_ref.clamp(e.pos, px(e.body_w) / 2, px(e.body_h) / 2);
    let room = level_ref.room_at(e.pos);
    write_back(world, eid, e);

    if room != world.players[pi].room {
        let (cx, cy) = camera_for_room(&world.levels[level as usize], room);
        let p = &mut world.players[pi];
        p.room = room;
        p.camera.target_x = cx;
        p.camera.target_y = cy;
        p.camera.scroll = ROOM_SCROLL_FRAMES;
    }
}

fn write_back(world: &mut World, eid: EntityId, e: Entity) {
    if let Some(slot) = world.entities.get_mut(eid) {
        *slot = e;
    }
}

/// Hurts every hero standing next to the boss.
fn slam(world: &mut World, pi: usize, at: V2) {
    let level = world.players[pi].level;
    let area = crate::geom::Rect::centered(at, 52, 48);
    let hits: Vec<usize> = world
        .entities
        .iter()
        .filter(|(_, e)| e.kind == Kind::Player && e.level == level && e.hurt_box().overlaps(&area))
        .map(|(_, e)| e.player as usize)
        .collect();
    for target in hits {
        if target != pi {
            world.hurt_player(target, 4, at);
        }
    }
    let id = world.spawn(Kind::Poof, level, at);
    if let Some(p) = world.entities.get_mut(id) {
        p.timer = 14;
    }
    world.events.sound(Sfx::Explosion);
    world.events.push(crate::event::Event::Shake { frames: 8 });
}

/// Throws fireballs out in every direction.
fn ring_of_fire(world: &mut World, boss: EntityId) {
    let Some(e) = world.entities.get(boss).cloned() else {
        return;
    };
    for i in 0..8 {
        let angle = i * 32;
        let id = world.spawn(Kind::Fireball, e.level, e.pos);
        if let Some(f) = world.entities.get_mut(id) {
            let sp = info(Kind::Fireball).speed;
            f.vel = V2::new(
                crate::fixed::cos(angle) * sp / 256,
                crate::fixed::sin(angle) * sp / 256,
            );
            f.timer = 120;
            f.owner = boss;
        }
    }
    world.events.sound(Sfx::Shoot);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{Level, LevelKind};
    use crate::tiles::tile;

    fn dungeon_world() -> World {
        let mut lv = Level::new(LevelKind::Dungeon, 2, 2, tile::FLOOR);
        lv.entrance = V2::from_px(80, 64);
        lv.spawns.push(Spawn {
            room: 3,
            kind: Kind::Boss as u8,
            tx: 15,
            ty: 12,
            param: 0,
        });
        let mut w = World::new(5150, vec![lv], 2);
        w.join(0);
        w
    }

    #[test]
    fn a_player_can_take_over_a_boss() {
        let mut w = dungeon_world();
        assert!(w.possess_boss(1));
        assert_eq!(w.players[1].role, Role::Boss);
        let e = w.player_entity(1).unwrap();
        assert_eq!(e.kind, Kind::Boss);
        assert!(e.has(eflag::POSSESSED));
    }

    #[test]
    fn a_possessed_boss_moves_on_command_not_on_its_own() {
        let mut w = dungeon_world();
        w.possess_boss(1);
        let start = w.player_entity(1).unwrap().pos;
        for _ in 0..30 {
            w.set_input(1, 0);
            w.step();
        }
        assert_eq!(
            w.player_entity(1).unwrap().pos,
            start,
            "a boss with no input should stand still"
        );
        for _ in 0..30 {
            w.set_input(1, button::LEFT);
            w.step();
        }
        assert!(w.player_entity(1).unwrap().pos.x < start.x);
    }

    #[test]
    fn a_boss_slam_hurts_a_hero_standing_next_to_it() {
        let mut w = dungeon_world();
        w.possess_boss(1);
        let boss_pos = w.player_entity(1).unwrap().pos;
        if let Some(hero) = w.entities.get_mut(w.players[0].entity) {
            hero.pos = V2::new(boss_pos.x + px(14), boss_pos.y);
            hero.level = w.players[1].level;
            hero.iframes = 0;
        }
        w.players[0].level = w.players[1].level;
        let hp0 = w.player_entity(0).unwrap().hp;
        w.set_input(1, button::A);
        w.step();
        assert!(w.player_entity(0).unwrap().hp < hp0);
    }

    #[test]
    fn a_boss_can_throw_a_ring_of_fire() {
        let mut w = dungeon_world();
        w.possess_boss(1);
        w.set_input(1, button::B);
        w.step();
        let fireballs = w
            .entities
            .iter()
            .filter(|(_, e)| e.kind == Kind::Fireball)
            .count();
        assert_eq!(fireballs, 8);
    }

    #[test]
    fn heroes_can_still_hurt_a_player_driven_boss() {
        let mut w = dungeon_world();
        w.possess_boss(1);
        let boss = w.players[1].entity;
        let hp0 = w.entities.get(boss).unwrap().hp;
        let at = w.entities.get(boss).unwrap().pos;
        w.damage(boss, 3, V2::new(at.x - px(20), at.y), 4);
        assert!(w.entities.get(boss).unwrap().hp < hp0);
    }

    #[test]
    fn killing_a_possessed_boss_returns_that_player_to_being_a_hero() {
        let mut w = dungeon_world();
        w.possess_boss(1);
        let boss = w.players[1].entity;
        w.kill(boss);
        w.set_input(1, 0);
        w.step();
        assert_eq!(w.players[1].role, Role::Hero);
        let e = w.player_entity(1).expect("should be a hero again");
        assert_eq!(e.kind, Kind::Player);
    }

    #[test]
    fn a_world_with_no_boss_refuses_possession() {
        let lv = Level::new(LevelKind::Overworld, 1, 1, tile::GRASS);
        let mut w = World::new(1, vec![lv], 1);
        w.join(0);
        assert!(!w.possess_boss(0));
        assert_eq!(w.players[0].role, Role::Hero);
    }

    #[test]
    fn possession_is_deterministic() {
        let mut a = dungeon_world();
        let mut b = dungeon_world();
        a.possess_boss(1);
        b.possess_boss(1);
        for f in 0..200u16 {
            let buttons = (f * 7) & 0x3f;
            a.set_input(1, buttons);
            b.set_input(1, buttons);
            a.set_input(0, button::RIGHT);
            b.set_input(0, button::RIGHT);
            a.step();
            b.step();
        }
        assert_eq!(a.checksum(), b.checksum());
    }
}
