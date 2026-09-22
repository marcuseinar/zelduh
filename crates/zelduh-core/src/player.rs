//! The player state machine: walking, swinging, lifting, falling, drowning.

use crate::entity::{eflag, Entity, EntityId, Kind};
use crate::event::{Event, Sfx};
use crate::fixed::{px, to_px, Fx, ONE};
use crate::geom::{Dir, V2};
use crate::input::button;
use crate::items::Item;
use crate::level::{tile_center, TILE_PX};
use crate::physics;
use crate::tiles::{self, flag, tile};
use crate::world::{camera_for_room, pstate, World, ROOM_SCROLL_FRAMES};

/// Walking speed in fixed-point pixels per frame.
const WALK: Fx = 320;
/// Diagonal movement is scaled so speed is the same in every direction.
const DIAG: Fx = 181;
/// Frames a sword swing lasts.
const SWING_FRAMES: u16 = 16;
/// Frames a lift takes before the object is overhead.
const LIFT_FRAMES: u16 = 10;
/// Upward speed of a feather jump.
const JUMP_VZ: Fx = 680;
/// Gravity applied to jumps and thrown objects, per frame.
pub const GRAVITY: Fx = 56;

/// Advances one player by a frame.
pub(crate) fn update(world: &mut World, pi: usize) {
    // A player who has taken over a boss is driving a monster, not a hero.
    if world.players[pi].role == crate::world::Role::Boss {
        crate::boss::update(world, pi);
        return;
    }
    let eid = world.players[pi].entity;
    let Some(mut e) = world.entities.get(eid).cloned() else {
        return;
    };

    e.iframes = e.iframes.saturating_sub(1);
    e.stun = e.stun.saturating_sub(1);
    if e.anim_timer > 0 {
        e.anim_timer -= 1;
    }

    if !world.players[pi].is_alive() || e.state == pstate::DEAD {
        e.vel = V2::ZERO;
        write_back(world, eid, e);
        return;
    }

    let input = world.players[pi].input;
    let level = e.level;

    match e.state {
        pstate::HURT => {
            e.timer = e.timer.saturating_sub(1);
            if e.timer == 0 {
                e.state = pstate::NORMAL;
            }
        }
        pstate::ATTACK | pstate::LIFT | pstate::THROW => {
            e.timer = e.timer.saturating_sub(1);
            if e.timer == 0 {
                e.state = pstate::NORMAL;
            }
        }
        pstate::FALL => {
            e.timer = e.timer.saturating_sub(1);
            if e.timer == 0 {
                e.state = pstate::NORMAL;
                let safe = V2::new(e.data[0], e.data[1]);
                e.pos = safe;
                e.z = 0;
                e.vz = 0;
                write_back(world, eid, e);
                // No knockback: the player has just been put back on solid
                // ground and must not be shoved into the hole again.
                let safe_pos = safe;
                world.hurt_player_with(pi, 2, safe_pos, 0);
                return;
            }
            write_back(world, eid, e);
            return;
        }
        _ => {}
    }

    // Knockback overrides steering for a few frames.
    if e.knock_frames > 0 {
        e.knock_frames -= 1;
        let k = e.knock;
        physics::move_by(&world.levels[level as usize].map, &mut e, k);
        if e.knock_frames == 0 {
            e.knock = V2::ZERO;
        }
    }

    let can_steer = e.state == pstate::NORMAL || e.state == pstate::SWIM || e.has(eflag::AIRBORNE);
    let (mut ax, mut ay) = (input.axis_x(), input.axis_y());
    if !can_steer {
        ax = 0;
        ay = 0;
    }

    if ax != 0 || ay != 0 {
        e.dir = facing_for(e.dir, ax, ay);
        e.anim_timer = e.anim_timer.max(1);
        if e.anim_timer == 1 {
            e.anim = (e.anim + 1) & 3;
            e.anim_timer = 7;
        }
    } else {
        e.anim = 0;
    }

    // Speed depends on terrain and state.
    let foot_tile = world.levels[level as usize].map.at_world(e.pos);
    let mut speed = WALK;
    if e.state == pstate::SWIM {
        speed = WALK / 2;
    } else if tiles::any(foot_tile, flag::SLOW) {
        speed = WALK * 3 / 4;
    }
    if e.state == pstate::ATTACK {
        speed /= 2;
    }
    if world.players[pi].inv.has(Item::Boots) && input.held(button::SELECT) {
        speed *= 2;
    }

    let mut delta = V2::new(speed * ax, speed * ay);
    if ax != 0 && ay != 0 {
        delta = V2::new(delta.x * DIAG / 256, delta.y * DIAG / 256);
    }

    // Airborne movement keeps going in the jump direction.
    if e.has(eflag::AIRBORNE) {
        e.vz -= GRAVITY;
        e.z += e.vz;
        if e.z <= 0 {
            e.z = 0;
            e.vz = 0;
            e.set_flag(eflag::AIRBORNE, false);
            if e.state == pstate::JUMP {
                e.state = pstate::NORMAL;
            }
        }
    }

    if delta.x != 0 || delta.y != 0 {
        physics::move_by(&world.levels[level as usize].map, &mut e, delta);
    }

    // Terrain the player is standing on now.
    let (tx, ty) = physics::tile_coords(e.pos);
    let here = world.levels[level as usize].map.get(tx, ty);
    let here_flags = tiles::flags(here);

    if !e.has(eflag::AIRBORNE) {
        if here_flags & flag::WATER != 0 {
            if here == tile::LAVA || !world.players[pi].inv.has(Item::Flippers) {
                // Wash back to the last safe spot and take a knock.
                let safe = V2::new(e.data[0], e.data[1]);
                e.pos = safe;
                e.state = pstate::NORMAL;
                write_back(world, eid, e);
                world.events.sound(Sfx::Splash);
                world.hurt_player_with(pi, 2, safe, 0);
                return;
            }
            e.state = pstate::SWIM;
        } else {
            if e.state == pstate::SWIM {
                e.state = pstate::NORMAL;
            }
            if here_flags & flag::PIT != 0 {
                e.state = pstate::FALL;
                e.timer = 24;
                world.events.sound(Sfx::Fall);
                write_back(world, eid, e);
                return;
            }
            if here_flags & (flag::SOLID | flag::HARMFUL) == 0 {
                // Remember where it is safe to be put back.
                e.data[0] = e.pos.x;
                e.data[1] = e.pos.y;
            }
        }

        // Hop down a ledge when walking into it.
        if let Some(ld) = tiles::ledge_dir(here) {
            if (ax, ay) == ld.step() || e.state == pstate::NORMAL && ld.step() == (ax, ay) {
                e.set_flag(eflag::AIRBORNE, true);
                e.vz = JUMP_VZ / 2;
                e.state = pstate::JUMP;
                let u = ld.unit();
                e.knock = V2::new(u.x * 2, u.y * 2);
                e.knock_frames = 12;
                world.events.sound(Sfx::Jump);
            }
        }
    }

    // Stairs and warps.
    if here_flags & flag::TRANSITION != 0 {
        if let Some(link) = world.levels[level as usize].link_at(tx, ty).copied() {
            write_back(world, eid, e);
            world.warp_player(pi, link.to_level, link.to_pos, link.to_dir);
            return;
        }
    }

    // Buttons.
    if input.pressed(button::A) {
        write_back(world, eid, e.clone());
        if try_interact(world, pi) {
            return;
        }
        let item = world.players[pi].inv.equipped[0];
        use_item(world, pi, item, 0);
        if let Some(updated) = world.entities.get(eid).cloned() {
            e = updated;
        }
    }
    if input.pressed(button::B) {
        write_back(world, eid, e.clone());
        let item = world.players[pi].inv.equipped[1];
        use_item(world, pi, item, 1);
        if let Some(updated) = world.entities.get(eid).cloned() {
            e = updated;
        }
    }
    if input.pressed(button::START) {
        world.players[pi].inv.swap_equipped();
    }

    // Keep the carried object glued overhead.
    let carrying = world.players[pi].carrying;
    if let Some(c) = world.entities.get_mut(carrying) {
        if c.state == 0 {
            c.pos = V2::new(e.pos.x, e.pos.y - px(4));
            c.z = px(14);
            c.level = e.level;
        }
    }

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
        world.events.push(Event::RoomChanged {
            player: pi as u8,
            rx: room.0,
            ry: room.1,
        });
    }
}

fn write_back(world: &mut World, eid: EntityId, e: Entity) {
    if let Some(slot) = world.entities.get_mut(eid) {
        *slot = e;
    }
}

/// Chooses a facing from the pressed axes, keeping the current one when it is
/// still being held so that diagonal movement does not flip-flop.
fn facing_for(cur: Dir, ax: i32, ay: i32) -> Dir {
    let horizontal = match ax {
        -1 => Some(Dir::Left),
        1 => Some(Dir::Right),
        _ => None,
    };
    let vertical = match ay {
        -1 => Some(Dir::Up),
        1 => Some(Dir::Down),
        _ => None,
    };
    match (horizontal, vertical) {
        (Some(h), Some(v)) => {
            if cur == h || cur == v {
                cur
            } else {
                h
            }
        }
        (Some(h), None) => h,
        (None, Some(v)) => v,
        (None, None) => cur,
    }
}

/// Tile coordinates directly in front of an entity.
pub fn tile_in_front(e: &Entity) -> (i32, i32) {
    let (dx, dy) = e.dir.step();
    let ahead = V2::new(
        e.pos.x + px(dx * (TILE_PX * 3 / 4)),
        e.pos.y + px(dy * (TILE_PX * 3 / 4)),
    );
    physics::tile_coords(ahead)
}

/// Handles context actions that take priority over the equipped item:
/// throwing what you are holding, opening a chest, unlocking a door, reading a
/// sign. Returns true when something happened.
fn try_interact(world: &mut World, pi: usize) -> bool {
    let carrying = world.players[pi].carrying;
    if !carrying.is_none() {
        throw_carried(world, pi);
        return true;
    }

    let Some(e) = world.player_entity(pi).cloned() else {
        return false;
    };
    let (tx, ty) = tile_in_front(&e);
    let t = world.level(e.level).map.get(tx, ty);

    if tiles::any(t, flag::LOCKED) {
        if world.players[pi].inv.use_key() {
            world.level_mut(e.level).map.set(tx, ty, tile::DOOR_OPEN);
            world.events.sound(Sfx::Unlock);
            world.say(pi, "THE DOOR OPENS");
        } else {
            world.events.sound(Sfx::Error);
            world.say(pi, "IT IS LOCKED");
        }
        return true;
    }
    if t == tile::SIGN {
        world.events.sound(Sfx::Text);
        world.say(pi, "DANGER AHEAD");
        return true;
    }

    // A chest standing in front of the player.
    let front = tile_center(tx, ty);
    let chest = world
        .entities
        .iter()
        .find(|(_, c)| {
            c.kind == Kind::Chest
                && c.level == e.level
                && c.state == 0
                && (c.pos.x - front.x).abs() < px(12)
                && (c.pos.y - front.y).abs() < px(12)
        })
        .map(|(id, c)| (id, c.data[0]));
    if let Some((id, param)) = chest {
        if let Some(c) = world.entities.get_mut(id) {
            c.state = 1;
        }
        world.events.sound(Sfx::ChestOpen);
        open_chest(world, pi, param);
        return true;
    }
    false
}

/// Grants the contents of a chest.
fn open_chest(world: &mut World, pi: usize, param: i32) {
    let (item, text): (Option<Item>, &'static str) = match param {
        1 => (Some(Item::Bombs), "BOMBS!"),
        2 => (Some(Item::Bow), "BOW AND ARROWS!"),
        3 => (Some(Item::Boomerang), "BOOMERANG!"),
        4 => (Some(Item::Feather), "ROC'S FEATHER!"),
        5 => (Some(Item::Bracelet), "POWER BRACELET!"),
        6 => (Some(Item::Flippers), "FLIPPERS!"),
        7 => (Some(Item::Boots), "PEGASUS BOOTS!"),
        8 => (Some(Item::Shield), "SHIELD!"),
        9 => (None, "A SMALL KEY"),
        _ => (None, "20 RUPEES"),
    };
    let p = &mut world.players[pi];
    match (item, param) {
        (Some(Item::Bombs), _) => {
            p.inv.give(Item::Bombs);
            p.inv.add_bombs(10);
        }
        (Some(Item::Bow), _) => {
            p.inv.give(Item::Bow);
            p.inv.add_arrows(20);
        }
        (Some(i), _) => p.inv.give(i),
        (None, 9) => p.inv.keys = p.inv.keys.saturating_add(1),
        (None, _) => p.inv.add_rupees(20),
    }
    world.say(pi, text);
}

/// Uses an equipped item. `slot` is 0 for A and 1 for B.
pub fn use_item(world: &mut World, pi: usize, item: Item, slot: usize) {
    let Some(e) = world.player_entity(pi).cloned() else {
        return;
    };
    if e.state == pstate::HURT || e.state == pstate::FALL {
        return;
    }
    if !world.players[pi].carrying.is_none() {
        throw_carried(world, pi);
        return;
    }
    if item != Item::None && !world.players[pi].inv.can_use(item) {
        world.events.sound(Sfx::Error);
        return;
    }
    let eid = world.players[pi].entity;
    match item {
        Item::Sword => swing_sword(world, pi),
        Item::Bombs => {
            world.players[pi].inv.spend(Item::Bombs);
            let (dx, dy) = e.dir.step();
            let at = V2::new(e.pos.x + px(dx * 14), e.pos.y + px(dy * 14));
            let id = world.spawn(Kind::Bomb, e.level, at);
            if let Some(b) = world.entities.get_mut(id) {
                b.timer = 120;
                b.owner = eid;
            }
            world.events.sound(Sfx::BombPlace);
        }
        Item::Bow => {
            world.players[pi].inv.spend(Item::Bow);
            let id = world.spawn(Kind::Arrow, e.level, e.pos);
            if let Some(a) = world.entities.get_mut(id) {
                a.dir = e.dir;
                a.owner = eid;
                a.flags |= eflag::PLAYER_OWNED;
                a.vel = e
                    .dir
                    .unit()
                    .with_length(crate::entity::info(Kind::Arrow).speed);
                a.timer = 60;
            }
            world.events.sound(Sfx::Shoot);
        }
        Item::Boomerang => {
            let already = world
                .entities
                .iter()
                .any(|(_, b)| b.kind == Kind::Boomerang && b.owner == eid);
            if already {
                return;
            }
            let id = world.spawn(Kind::Boomerang, e.level, e.pos);
            if let Some(b) = world.entities.get_mut(id) {
                b.dir = e.dir;
                b.owner = eid;
                b.vel = e
                    .dir
                    .unit()
                    .with_length(crate::entity::info(Kind::Boomerang).speed);
                b.timer = 26;
            }
            world.events.sound(Sfx::Boomerang);
        }
        Item::Feather => {
            if let Some(p) = world.entities.get_mut(eid) {
                if !p.has(eflag::AIRBORNE) {
                    p.set_flag(eflag::AIRBORNE, true);
                    p.vz = JUMP_VZ;
                    p.state = pstate::JUMP;
                }
            }
            world.events.sound(Sfx::Jump);
        }
        Item::Bracelet => {
            lift(world, pi);
        }
        Item::Shield => {
            world.events.sound(Sfx::Shield);
        }
        Item::Boots | Item::Flippers | Item::None => {
            // Passive, or nothing equipped: fall back to lifting.
            if slot == 0 {
                lift(world, pi);
            }
        }
    }
}

fn swing_sword(world: &mut World, pi: usize) {
    let eid = world.players[pi].entity;
    let Some(e) = world.entities.get(eid).cloned() else {
        return;
    };
    if e.state == pstate::ATTACK {
        return;
    }
    if let Some(p) = world.entities.get_mut(eid) {
        p.state = pstate::ATTACK;
        p.timer = SWING_FRAMES;
    }
    let (dx, dy) = e.dir.step();
    let at = V2::new(e.pos.x + px(dx * 12), e.pos.y + px(dy * 12) - px(4));
    let id = world.spawn(Kind::SwordSwing, e.level, at);
    if let Some(s) = world.entities.get_mut(id) {
        s.owner = eid;
        s.dir = e.dir;
        s.timer = SWING_FRAMES - 4;
        s.data[0] = world.players[pi].inv.sword_level.max(1) as i32;
    }
    world.events.sound(Sfx::SwordSwing);

    // At full health the sword throws a beam, as it does in the classics.
    if e.hp >= e.max_hp {
        let id = world.spawn(Kind::SwordBeam, e.level, at);
        if let Some(b) = world.entities.get_mut(id) {
            b.owner = eid;
            b.dir = e.dir;
            b.vel = e
                .dir
                .unit()
                .with_length(crate::entity::info(Kind::SwordBeam).speed);
            b.timer = 50;
        }
        world.events.sound(Sfx::SwordBeam);
    }
}

/// Picks up the tile or pot in front of the player.
fn lift(world: &mut World, pi: usize) {
    let Some(e) = world.player_entity(pi).cloned() else {
        return;
    };
    let (tx, ty) = tile_in_front(&e);
    let t = world.level(e.level).map.get(tx, ty);
    if !tiles::any(t, flag::LIFTABLE) {
        return;
    }
    // Rocks need the bracelet; bushes and pots do not.
    if t == tile::ROCK && !world.players[pi].inv.has(Item::Bracelet) {
        world.events.sound(Sfx::Error);
        world.say(pi, "TOO HEAVY");
        return;
    }
    world
        .level_mut(e.level)
        .map
        .set(tx, ty, tiles::destroyed(t));
    let eid = world.players[pi].entity;
    let id = world.spawn(Kind::Carried, e.level, tile_center(tx, ty));
    if let Some(c) = world.entities.get_mut(id) {
        c.owner = eid;
        c.data[0] = t as i32;
        c.z = px(14);
        c.state = 0;
    }
    world.players[pi].carrying = id;
    if let Some(p) = world.entities.get_mut(eid) {
        p.state = pstate::LIFT;
        p.timer = LIFT_FRAMES;
    }
    world.events.sound(Sfx::Lift);
}

/// Throws whatever the player is carrying.
fn throw_carried(world: &mut World, pi: usize) {
    let cid = world.players[pi].carrying;
    world.players[pi].carrying = EntityId::NONE;
    let Some(e) = world.player_entity(pi).cloned() else {
        return;
    };
    if let Some(c) = world.entities.get_mut(cid) {
        c.state = 1;
        c.vel = e.dir.unit().with_length(px(4));
        c.vz = 0;
        c.z = px(12);
        c.timer = 90;
    }
    if let Some(p) = world.entities.get_mut(world.players[pi].entity) {
        p.state = pstate::THROW;
        p.timer = 8;
    }
    world.events.sound(Sfx::Throw);
}

impl crate::world::Player {
    /// Centre of this player's viewport in world pixels, used as a fallback
    /// source position for damage that has no obvious origin.
    pub fn camera_center(&self) -> V2 {
        V2::new(
            self.camera.x + px(crate::world::VIEW_W / 2),
            self.camera.y + px(crate::world::VIEW_H / 2),
        )
    }
}

/// Height in whole pixels an entity is drawn above its shadow.
pub fn draw_z(e: &Entity) -> i32 {
    to_px(e.z).max(0)
}

#[allow(dead_code)]
fn unused_one() -> Fx {
    ONE
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{Level, LevelKind};
    use crate::world::World;

    fn world_with(fill: u8) -> World {
        let mut lv = Level::new(LevelKind::Overworld, 2, 2, fill);
        lv.entrance = V2::from_px(80, 64);
        let mut w = World::new(7, vec![lv], 1);
        w.join(0);
        w
    }

    #[test]
    fn walking_right_moves_right() {
        let mut w = world_with(tile::GRASS);
        let x0 = w.player_entity(0).unwrap().pos.x;
        for _ in 0..10 {
            w.set_input(0, button::RIGHT);
            w.step();
        }
        let e = w.player_entity(0).unwrap();
        assert!(e.pos.x > x0);
        assert_eq!(e.dir, Dir::Right);
    }

    #[test]
    fn diagonal_is_not_faster_than_straight() {
        let mut straight = world_with(tile::GRASS);
        let mut diag = world_with(tile::GRASS);
        for _ in 0..30 {
            straight.set_input(0, button::RIGHT);
            straight.step();
            diag.set_input(0, button::RIGHT | button::DOWN);
            diag.step();
        }
        let a = straight.player_entity(0).unwrap().pos;
        let b = diag.player_entity(0).unwrap().pos;
        let da = a.sub(V2::from_px(80, 64)).length();
        let db = b.sub(V2::from_px(80, 64)).length();
        assert!((da - db).abs() < px(3), "{da} vs {db}");
    }

    #[test]
    fn swinging_the_sword_spawns_a_blade() {
        let mut w = world_with(tile::GRASS);
        w.set_input(0, button::A);
        w.step();
        assert!(w.entities.iter().any(|(_, e)| e.kind == Kind::SwordSwing));
        assert!(w.events.has_sound(Sfx::SwordSwing));
        assert_eq!(w.player_entity(0).unwrap().state, pstate::ATTACK);
    }

    #[test]
    fn a_full_health_swing_throws_a_beam() {
        let mut w = world_with(tile::GRASS);
        w.set_input(0, button::A);
        w.step();
        assert!(w.entities.iter().any(|(_, e)| e.kind == Kind::SwordBeam));
    }

    #[test]
    fn a_wounded_swing_throws_no_beam() {
        let mut w = world_with(tile::GRASS);
        w.hurt_player(0, 2, V2::from_px(0, 0));
        for _ in 0..60 {
            w.set_input(0, 0);
            w.step();
        }
        w.set_input(0, button::A);
        w.step();
        assert!(!w.entities.iter().any(|(_, e)| e.kind == Kind::SwordBeam));
    }

    #[test]
    fn walking_into_deep_water_pushes_you_back_and_hurts() {
        let mut w = world_with(tile::GRASS);
        w.levels[0].map.fill_rect(6, 0, 4, 8, tile::WATER);
        let hp0 = w.player_entity(0).unwrap().hp;
        for _ in 0..40 {
            w.set_input(0, button::RIGHT);
            w.step();
        }
        let e = w.player_entity(0).unwrap();
        assert!(e.hp < hp0, "should have been hurt by the water");
        assert!(
            !tiles::any(w.levels[0].map.at_world(e.pos), flag::WATER),
            "should be back on dry land"
        );
    }

    #[test]
    fn flippers_let_you_swim() {
        let mut w = world_with(tile::GRASS);
        w.levels[0].map.fill_rect(6, 0, 4, 8, tile::WATER);
        w.players[0].inv.give(Item::Flippers);
        for _ in 0..40 {
            w.set_input(0, button::RIGHT);
            w.step();
        }
        let e = w.player_entity(0).unwrap();
        assert_eq!(e.hp, 12);
        assert_eq!(e.state, pstate::SWIM);
    }

    #[test]
    fn falling_in_a_pit_costs_health_and_moves_you_back() {
        let mut w = world_with(tile::FLOOR);
        w.levels[0].map.fill_rect(6, 0, 3, 8, tile::PIT);
        // Long enough to walk in, short enough that repeated falls do not kill
        // and respawn the player.
        for _ in 0..20 {
            w.set_input(0, button::RIGHT);
            w.step();
        }
        // Let the fall finish without walking straight back in.
        for _ in 0..40 {
            w.set_input(0, 0);
            w.step();
        }
        let e = w.player_entity(0).unwrap();
        assert!(e.hp < 12);
        assert!(
            !tiles::any(w.levels[0].map.at_world(e.pos), flag::PIT),
            "should have been placed back on solid ground"
        );
    }

    /// Stands the player in the middle of a tile so that walking into the
    /// next one along cannot slip past its corner.
    fn center_player_on_tile(w: &mut World, tx: i32, ty: i32) {
        let eid = w.players[0].entity;
        let c = crate::level::tile_center(tx, ty);
        let e = w.entities.get_mut(eid).unwrap();
        e.pos = c;
        e.data[0] = c.x;
        e.data[1] = c.y;
    }

    #[test]
    fn lifting_a_bush_removes_it_and_gives_you_something_to_throw() {
        let mut w = world_with(tile::GRASS);
        center_player_on_tile(&mut w, 5, 4);
        // Put a bush directly to the right of the player.
        let (tx, ty) = (6, 4);
        w.levels[0].map.set(tx, ty, tile::BUSH);
        w.players[0].inv.equipped[0] = Item::Bracelet;
        w.players[0].inv.give(Item::Bracelet);
        for _ in 0..20 {
            w.set_input(0, button::RIGHT);
            w.step();
        }
        w.set_input(0, button::RIGHT | button::A);
        w.step();
        assert_eq!(w.levels[0].map.get(tx, ty), tile::GRASS);
        assert!(!w.players[0].carrying.is_none());
        // Throwing it sends it flying.
        w.set_input(0, 0);
        w.step();
        w.set_input(0, button::A);
        w.step();
        assert!(
            w.players[0].carrying.is_none(),
            "it should have been thrown"
        );
    }

    #[test]
    fn a_locked_door_consumes_a_key() {
        let mut w = world_with(tile::FLOOR);
        center_player_on_tile(&mut w, 5, 4);
        let (tx, ty) = (6, 4);
        w.levels[0].map.set(tx, ty, tile::DOOR_LOCKED);
        w.players[0].inv.keys = 1;
        // Walk up against the door first: you have to be next to it.
        for _ in 0..20 {
            w.set_input(0, button::RIGHT);
            w.step();
        }
        w.set_input(0, button::RIGHT | button::A);
        w.step();
        assert_eq!(w.levels[0].map.get(tx, ty), tile::DOOR_OPEN);
        assert_eq!(w.players[0].inv.keys, 0);
    }

    #[test]
    fn crossing_a_room_boundary_scrolls_the_camera() {
        let mut w = world_with(tile::GRASS);
        assert_eq!(w.players[0].room, (0, 0));
        for _ in 0..200 {
            w.set_input(0, button::RIGHT);
            w.step();
            if w.players[0].room == (1, 0) {
                break;
            }
        }
        assert_eq!(w.players[0].room, (1, 0));
        assert!(w.players[0].camera.target_x > 0);
    }

    #[test]
    fn bombs_need_ammunition() {
        let mut w = world_with(tile::GRASS);
        w.players[0].inv.give(Item::Bombs);
        w.players[0].inv.equipped[1] = Item::Bombs;
        w.set_input(0, button::B);
        w.step();
        assert!(!w.entities.iter().any(|(_, e)| e.kind == Kind::Bomb));
        w.players[0].inv.add_bombs(1);
        w.set_input(0, 0);
        w.step();
        w.set_input(0, button::B);
        w.step();
        assert!(w.entities.iter().any(|(_, e)| e.kind == Kind::Bomb));
        assert_eq!(w.players[0].inv.bombs, 0);
    }
}
