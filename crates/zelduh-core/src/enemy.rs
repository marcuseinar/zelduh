//! Monster behaviour.
//!
//! Every monster is a small state machine driven by timers and the shared
//! deterministic RNG. None of them look at wall-clock time or at anything the
//! renderer does, so two machines running the same inputs see the same fight.

use crate::entity::{eflag, info, Entity, Kind};
use crate::fixed::px;
use crate::geom::{Dir, V2};
use crate::level::Map;
use crate::physics;
use crate::tiles::{self, flag};
use crate::world::World;

/// Advances one monster by a frame.
pub(crate) fn update(world: &mut World, idx: usize) {
    let id = world.entities.id_of(idx);
    let Some(mut e) = world.entities.at(idx).cloned() else {
        return;
    };

    e.iframes = e.iframes.saturating_sub(1);
    e.stun = e.stun.saturating_sub(1);
    e.timer = e.timer.saturating_sub(1);
    e.anim_timer = e.anim_timer.wrapping_add(1);
    if e.anim_timer % 8 == 0 {
        e.anim = (e.anim + 1) & 3;
    }

    // Monsters far from every player go to sleep rather than wandering off.
    if !world.is_near_player(e.level, e.pos, 1) {
        write(world, idx, e);
        return;
    }

    if e.knock_frames > 0 {
        e.knock_frames -= 1;
        let k = e.knock;
        let map = &world.levels[e.level as usize].map;
        physics::move_by(map, &mut e, k);
        if e.knock_frames == 0 {
            e.knock = V2::ZERO;
        }
        write(world, idx, e);
        return;
    }
    if e.stun > 0 {
        write(world, idx, e);
        return;
    }

    let target = world.nearest_player(e.level, e.pos, 200).map(|(_, p)| p);

    match e.kind {
        Kind::Octorok => octorok(world, &mut e, id, target),
        Kind::Moblin => moblin(world, &mut e, id, target),
        Kind::Zol => zol(world, &mut e, target),
        Kind::Keese => keese(world, &mut e, target),
        Kind::Tektite => tektite(world, &mut e, target),
        Kind::Stalfos => stalfos(world, &mut e, target),
        Kind::Boss => boss(world, &mut e, id, target),
        _ => {}
    }

    write(world, idx, e);
}

fn write(world: &mut World, idx: usize, e: Entity) {
    if let Some(slot) = world.entities.at_mut(idx) {
        *slot = e;
    }
}

/// Moves a walker, turning around when it would hit a wall or step into water.
fn walk(map: &Map, e: &mut Entity, dir: Dir, speed: i32) -> bool {
    let d = dir.unit().with_length(speed);
    let ahead = V2::new(e.pos.x + d.x * 3, e.pos.y + d.y * 3);
    if !e.has(eflag::FLYING) {
        let t = map.at_world(ahead);
        if tiles::any(t, flag::WATER | flag::PIT | flag::HARMFUL) {
            return false;
        }
    }
    let hit = physics::move_by(map, e, d);
    !hit.any()
}

/// Picks a random direction, biased towards the player when one is in sight.
fn choose_dir(world: &mut World, e: &Entity, target: Option<V2>, bias: u32) -> Dir {
    if let Some(t) = target {
        if world.rng.below(100) < bias {
            let d = t.sub(e.pos);
            return if d.x.abs() > d.y.abs() {
                if d.x < 0 {
                    Dir::Left
                } else {
                    Dir::Right
                }
            } else if d.y < 0 {
                Dir::Up
            } else {
                Dir::Down
            };
        }
    }
    Dir::from_u8(world.rng.below(4) as u8)
}

fn fire_at(world: &mut World, e: &Entity, kind: Kind, dir: Dir) {
    let id = world.spawn(kind, e.level, e.pos);
    if let Some(p) = world.entities.get_mut(id) {
        p.dir = dir;
        p.vel = dir.unit().with_length(info(kind).speed);
        p.timer = 90;
    }
    world.events.sound(crate::event::Sfx::Shoot);
}

fn octorok(world: &mut World, e: &mut Entity, _id: crate::entity::EntityId, target: Option<V2>) {
    match e.state {
        // Walking.
        0 => {
            let dir = e.dir;
            let speed = info(Kind::Octorok).speed;
            let moved = walk(&world.levels[e.level as usize].map, e, dir, speed);
            if e.timer == 0 {
                e.state = 1;
                e.timer = world.rng.range(20, 50) as u16;
            } else if !moved {
                // Blocked: face somewhere else and keep going.
                e.dir = choose_dir(world, e, target, 40);
            }
        }
        // Pausing, and maybe spitting a rock.
        _ => {
            if e.timer == 0 {
                if let Some(t) = target {
                    let d = t.sub(e.pos);
                    let aligned = d.x.abs() < px(20) || d.y.abs() < px(20);
                    if aligned && world.rng.below(100) < 60 {
                        e.dir = Dir::from_delta(d.x, d.y);
                        fire_at(world, e, Kind::Rock, e.dir);
                    }
                }
                e.state = 0;
                e.timer = world.rng.range(30, 80) as u16;
                e.dir = choose_dir(world, e, target, 40);
            }
        }
    }
}

fn moblin(world: &mut World, e: &mut Entity, _id: crate::entity::EntityId, target: Option<V2>) {
    let speed = info(Kind::Moblin).speed;
    let close = target
        .map(|t| t.sub(e.pos).length() < px(72))
        .unwrap_or(false);
    if e.timer == 0 {
        e.dir = choose_dir(world, e, target, if close { 85 } else { 25 });
        e.timer = world.rng.range(20, 50) as u16;
        // Moblins loose an arrow when they line up with a hero.
        if let Some(t) = target {
            let d = t.sub(e.pos);
            if (d.x.abs() < px(12) || d.y.abs() < px(12)) && world.rng.below(100) < 35 {
                e.dir = Dir::from_delta(d.x, d.y);
                fire_at(world, e, Kind::Arrow, e.dir);
                e.timer = 30;
                return;
            }
        }
    }
    let dir = e.dir;
    if !walk(&world.levels[e.level as usize].map, e, dir, speed) {
        e.timer = 0;
    }
}

fn zol(world: &mut World, e: &mut Entity, target: Option<V2>) {
    match e.state {
        0 => {
            if e.timer == 0 {
                e.state = 1;
                e.timer = 22;
                e.dir = choose_dir(world, e, target, 70);
                e.vz = 420;
                e.set_flag(eflag::AIRBORNE, true);
            }
        }
        _ => {
            e.vz -= crate::player::GRAVITY;
            e.z = (e.z + e.vz).max(0);
            if e.z <= 0 {
                e.z = 0;
                e.vz = 0;
                e.set_flag(eflag::AIRBORNE, false);
                e.state = 0;
                e.timer = world.rng.range(30, 70) as u16;
            }
            let dir = e.dir;
            let speed = info(Kind::Zol).speed * 2;
            walk(&world.levels[e.level as usize].map, e, dir, speed);
        }
    }
}

fn keese(world: &mut World, e: &mut Entity, target: Option<V2>) {
    // Bats rest until disturbed, then flap about in bursts.
    if e.state == 0 {
        let awake = target
            .map(|t| t.sub(e.pos).length() < px(56))
            .unwrap_or(false);
        if awake {
            e.state = 1;
            e.timer = 40;
        }
        return;
    }
    if e.timer == 0 {
        e.timer = world.rng.range(20, 60) as u16;
        let angle = world.rng.below(256) as i32;
        let speed = info(Kind::Keese).speed;
        e.vel = V2::new(
            crate::fixed::cos(angle) * speed / 256,
            crate::fixed::sin(angle) * speed / 256,
        );
        e.dir = Dir::from_delta(e.vel.x, e.vel.y);
    }
    let v = e.vel;
    let map = &world.levels[e.level as usize].map;
    let hit = physics::move_by(map, e, v);
    if hit.x {
        e.vel.x = -e.vel.x;
    }
    if hit.y {
        e.vel.y = -e.vel.y;
    }
}

fn tektite(world: &mut World, e: &mut Entity, target: Option<V2>) {
    match e.state {
        0 => {
            if e.timer == 0 {
                e.state = 1;
                e.timer = 30;
                e.vz = 600;
                e.set_flag(eflag::AIRBORNE, true);
                let dir = choose_dir(world, e, target, 65);
                e.dir = dir;
                let speed = info(Kind::Tektite).speed;
                e.vel = dir.unit().with_length(speed);
            }
        }
        _ => {
            e.vz -= crate::player::GRAVITY;
            e.z = (e.z + e.vz).max(0);
            let v = e.vel;
            let map = &world.levels[e.level as usize].map;
            let hit = physics::move_by(map, e, v);
            if hit.any() {
                e.vel = V2::new(-e.vel.x, -e.vel.y);
            }
            if e.z <= 0 {
                e.z = 0;
                e.vz = 0;
                e.vel = V2::ZERO;
                e.set_flag(eflag::AIRBORNE, false);
                e.state = 0;
                e.timer = world.rng.range(20, 50) as u16;
            }
        }
    }
}

fn stalfos(world: &mut World, e: &mut Entity, target: Option<V2>) {
    let speed = info(Kind::Stalfos).speed;
    // Skeletons dodge away when a hero gets right on top of them.
    let very_close = target
        .map(|t| t.sub(e.pos).length() < px(24))
        .unwrap_or(false);
    if very_close && e.state != 2 {
        e.state = 2;
        e.timer = 18;
        if let Some(t) = target {
            e.dir = Dir::from_delta(t.x - e.pos.x, t.y - e.pos.y).opposite();
        }
    }
    match e.state {
        2 => {
            let dir = e.dir;
            walk(&world.levels[e.level as usize].map, e, dir, speed * 3 / 2);
            if e.timer == 0 {
                e.state = 0;
                e.timer = 20;
            }
        }
        _ => {
            if e.timer == 0 {
                e.dir = choose_dir(world, e, target, 75);
                e.timer = world.rng.range(18, 40) as u16;
            }
            let dir = e.dir;
            if !walk(&world.levels[e.level as usize].map, e, dir, speed) {
                e.timer = 0;
            }
        }
    }
}

/// The dungeon boss: charges, then rings the room with fireballs.
fn boss(world: &mut World, e: &mut Entity, _id: crate::entity::EntityId, target: Option<V2>) {
    let speed = info(Kind::Boss).speed;
    // Below half health it gets angry and moves faster.
    let enraged = e.hp * 2 <= e.max_hp;
    let speed = if enraged { speed * 3 / 2 } else { speed };

    match e.state {
        // Stalk the nearest hero.
        0 => {
            if e.timer == 0 {
                e.state = if world.rng.coin() { 1 } else { 0 };
                e.timer = world.rng.range(40, 90) as u16;
                e.dir = choose_dir(world, e, target, 80);
            }
            let dir = e.dir;
            if !walk(&world.levels[e.level as usize].map, e, dir, speed) {
                e.timer = 0;
            }
        }
        // Wind up, then spit a ring of fire.
        1 => {
            if e.timer == 0 {
                let count = if enraged { 8 } else { 4 };
                for i in 0..count {
                    let angle = i * 256 / count + if enraged { 16 } else { 0 };
                    let id = world.spawn(Kind::Fireball, e.level, e.pos);
                    if let Some(f) = world.entities.get_mut(id) {
                        let sp = info(Kind::Fireball).speed;
                        f.vel = V2::new(
                            crate::fixed::cos(angle) * sp / 256,
                            crate::fixed::sin(angle) * sp / 256,
                        );
                        f.timer = 120;
                    }
                }
                world.events.sound(crate::event::Sfx::Shoot);
                e.state = 0;
                e.timer = world.rng.range(60, 110) as u16;
            }
        }
        _ => {
            e.state = 0;
            e.timer = 30;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{Level, LevelKind};
    use crate::tiles::tile;

    fn arena(fill: u8) -> World {
        arena_sized(fill, 2, 2)
    }

    fn arena_sized(fill: u8, w: i32, h: i32) -> World {
        let mut lv = Level::new(LevelKind::Overworld, w, h, fill);
        lv.entrance = V2::from_px(80, 64);
        let mut w = World::new(31, vec![lv], 1);
        w.join(0);
        w
    }

    #[test]
    fn an_octorok_wanders() {
        let mut w = arena(tile::GRASS);
        let id = w.spawn(Kind::Octorok, 0, V2::from_px(100, 64));
        let start = w.entities.get(id).unwrap().pos;
        for _ in 0..180 {
            w.step();
        }
        let now = w.entities.get(id).unwrap().pos;
        assert_ne!(start, now, "the octorok should have moved");
    }

    #[test]
    fn monsters_stay_out_of_the_water() {
        let mut w = arena(tile::GRASS);
        w.levels[0].map.fill_rect(0, 0, 20, 8, tile::WATER);
        w.levels[0].map.fill_rect(5, 3, 3, 3, tile::GRASS);
        let id = w.spawn(Kind::Octorok, 0, crate::level::tile_center(6, 4));
        for _ in 0..300 {
            w.step();
            let Some(e) = w.entities.get(id) else { break };
            let t = w.levels[0].map.at_world(e.pos);
            assert!(
                !tiles::any(t, flag::WATER),
                "walked into water at {:?}",
                e.pos
            );
        }
    }

    #[test]
    fn an_octorok_spits_rocks_at_a_hero() {
        let mut w = arena(tile::GRASS);
        let pos = w.player_entity(0).unwrap().pos;
        w.spawn(Kind::Octorok, 0, V2::new(pos.x + px(48), pos.y));
        let mut saw_rock = false;
        for _ in 0..600 {
            w.step();
            if w.entities.iter().any(|(_, e)| e.kind == Kind::Rock) {
                saw_rock = true;
                break;
            }
        }
        assert!(saw_rock, "an aligned octorok should eventually shoot");
    }

    #[test]
    fn a_boss_throws_fireballs() {
        let mut w = arena(tile::FLOOR);
        let pos = w.player_entity(0).unwrap().pos;
        let id = w.spawn(Kind::Boss, 0, V2::new(pos.x + px(40), pos.y));
        w.entities.get_mut(id).unwrap().state = 1;
        w.entities.get_mut(id).unwrap().timer = 1;
        let mut saw = false;
        for _ in 0..400 {
            w.step();
            if w.entities.iter().any(|(_, e)| e.kind == Kind::Fireball) {
                saw = true;
                break;
            }
        }
        assert!(saw);
    }

    #[test]
    fn distant_monsters_do_not_move() {
        let mut w = arena_sized(tile::GRASS, 4, 4);
        // Three rooms away, well outside the active window.
        let id = w.spawn(Kind::Octorok, 0, V2::from_px(500, 400));
        let start = w.entities.get(id).unwrap().pos;
        for _ in 0..60 {
            w.step();
        }
        // It is culled or asleep, but it must not have wandered.
        if let Some(e) = w.entities.get(id) {
            assert_eq!(e.pos, start);
        }
    }
}
