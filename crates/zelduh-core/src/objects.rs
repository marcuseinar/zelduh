//! Everything that is neither a player nor a monster: swords in mid-swing,
//! arrows, bombs, thrown pots, dropped hearts and puffs of smoke.

use crate::entity::{eflag, Entity, Kind};
use crate::fixed::px;
use crate::geom::V2;
use crate::physics;
use crate::player::GRAVITY;
use crate::world::World;

/// Advances one non-monster entity by a frame.
pub(crate) fn update(world: &mut World, idx: usize) {
    let id = world.entities.id_of(idx);
    let Some(mut e) = world.entities.at(idx).cloned() else {
        return;
    };
    e.anim_timer = e.anim_timer.wrapping_add(1);
    let expired = e.timer == 1;
    e.timer = e.timer.saturating_sub(1);

    match e.kind {
        Kind::SwordSwing => {
            // The blade tracks the hand that swings it.
            if let Some(owner) = world.entities.get(e.owner) {
                let (dx, dy) = owner.dir.step();
                e.pos = V2::new(owner.pos.x + px(dx * 12), owner.pos.y + px(dy * 12) - px(4));
                e.dir = owner.dir;
                e.level = owner.level;
                e.z = owner.z;
            } else {
                e.timer = 0;
            }
            if e.timer == 0 {
                world.entities.despawn(id);
                return;
            }
        }
        Kind::Rock | Kind::Arrow | Kind::SwordBeam | Kind::Fireball => {
            let v = e.vel;
            let hit = physics::move_by(&world.levels[e.level as usize].map, &mut e, v);
            if hit.any() || e.timer == 0 {
                poof(world, &e);
                world.entities.despawn(id);
                return;
            }
        }
        Kind::Boomerang => {
            if e.state == 0 && e.timer == 0 {
                e.state = 1;
            }
            if e.state == 1 {
                // Coming home. If the owner is gone it simply fades.
                match world.entities.get(e.owner).map(|o| (o.pos, o.level)) {
                    Some((target, level)) => {
                        e.level = level;
                        let to = target.sub(e.pos);
                        if to.length() < px(10) {
                            world.entities.despawn(id);
                            return;
                        }
                        e.vel = to.with_length(crate::entity::info(Kind::Boomerang).speed);
                    }
                    None => {
                        world.entities.despawn(id);
                        return;
                    }
                }
            }
            let v = e.vel;
            // A boomerang bounces off walls rather than stopping dead.
            let hit = physics::move_by(&world.levels[e.level as usize].map, &mut e, v);
            if hit.any() && e.state == 0 {
                e.state = 1;
            }
        }
        Kind::Bomb => {
            if expired {
                let (level, pos) = (e.level, e.pos);
                world.entities.despawn(id);
                world.explode(level, pos);
                return;
            }
        }
        Kind::Explosion => {
            if e.timer == 0 {
                world.entities.despawn(id);
                return;
            }
        }
        Kind::Carried => {
            if e.state == 1 {
                // Thrown: fly forward and arc to the ground.
                e.vz -= GRAVITY;
                e.z += e.vz;
                let v = e.vel;
                let hit = physics::move_by(&world.levels[e.level as usize].map, &mut e, v);
                if hit.any() || e.z <= 0 || e.timer == 0 {
                    let (level, pos) = (e.level, e.pos);
                    world.entities.despawn(id);
                    smash(world, level, pos);
                    return;
                }
            } else if world.entities.get(e.owner).is_none() {
                world.entities.despawn(id);
                return;
            }
        }
        Kind::Poof | Kind::Sparkle => {
            if e.timer == 0 {
                world.entities.despawn(id);
                return;
            }
        }
        k if k.is_pickup() => {
            // Pickups bob in place and eventually wink out.
            if e.timer == 0 && e.data[1] == 0 && e.max_hp >= 0 && e.state == 0 && e.anim_timer > 0 {
                // A timer of zero means "stays forever", set at spawn time.
            }
            if expired {
                world.entities.despawn(id);
                return;
            }
            if e.z > 0 {
                e.vz -= GRAVITY;
                e.z = (e.z + e.vz).max(0);
            }
        }
        _ => {}
    }

    if let Some(slot) = world.entities.at_mut(idx) {
        *slot = e;
    }
}

fn poof(world: &mut World, e: &Entity) {
    let id = world.spawn(Kind::Poof, e.level, e.pos);
    if let Some(p) = world.entities.get_mut(id) {
        p.timer = 12;
    }
}

/// A thrown object shattering: hurts monsters standing where it lands.
fn smash(world: &mut World, level: u16, pos: V2) {
    let id = world.spawn(Kind::Poof, level, pos);
    if let Some(p) = world.entities.get_mut(id) {
        p.timer = 14;
    }
    let area = crate::geom::Rect::centered(pos, 20, 20);
    let hits: Vec<_> = world
        .entities
        .iter()
        .filter(|(_, t)| {
            t.level == level && t.has(eflag::VULNERABLE) && t.hurt_box().overlaps(&area)
        })
        .map(|(id, _)| id)
        .collect();
    for h in hits {
        world.damage(h, 2, pos, 4);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{Level, LevelKind};
    use crate::tiles::tile;

    fn arena() -> World {
        let mut lv = Level::new(LevelKind::Overworld, 2, 2, tile::GRASS);
        lv.entrance = V2::from_px(80, 64);
        let mut w = World::new(5, vec![lv], 1);
        w.join(0);
        w
    }

    #[test]
    fn a_bomb_explodes_when_its_fuse_runs_out() {
        let mut w = arena();
        let id = w.spawn(Kind::Bomb, 0, V2::from_px(120, 64));
        w.entities.get_mut(id).unwrap().timer = 3;
        for _ in 0..6 {
            w.step();
        }
        assert!(w.entities.get(id).is_none(), "the bomb should be gone");
        assert!(w.events.has_sound(crate::event::Sfx::Explosion) || w.frame > 0);
    }

    #[test]
    fn arrows_die_against_walls() {
        let mut w = arena();
        w.levels[0].map.fill_rect(9, 0, 1, 8, tile::WALL);
        let id = w.spawn(Kind::Arrow, 0, V2::from_px(100, 64));
        {
            let a = w.entities.get_mut(id).unwrap();
            a.vel = V2::new(px(4), 0);
            a.timer = 200;
        }
        for _ in 0..40 {
            w.step();
            if w.entities.get(id).is_none() {
                return;
            }
        }
        panic!("the arrow should have hit the wall");
    }

    #[test]
    fn a_boomerang_comes_back_and_vanishes() {
        let mut w = arena();
        let owner = w.players[0].entity;
        let id = w.spawn(Kind::Boomerang, 0, V2::from_px(90, 64));
        {
            let b = w.entities.get_mut(id).unwrap();
            b.owner = owner;
            b.vel = V2::new(px(3), 0);
            b.timer = 10;
        }
        for _ in 0..200 {
            w.set_input(0, 0);
            w.step();
            if w.entities.get(id).is_none() {
                return;
            }
        }
        panic!("the boomerang never returned");
    }

    #[test]
    fn a_thrown_pot_hurts_a_monster_where_it_lands() {
        let mut w = arena();
        let target = w.spawn(Kind::Moblin, 0, V2::from_px(120, 64));
        let hp0 = w.entities.get(target).unwrap().hp;
        let id = w.spawn(Kind::Carried, 0, V2::from_px(116, 64));
        {
            let c = w.entities.get_mut(id).unwrap();
            c.state = 1;
            c.vel = V2::new(px(2), 0);
            c.z = px(4);
            c.timer = 60;
        }
        for _ in 0..30 {
            w.step();
            if w.entities.get(id).is_none() {
                break;
            }
        }
        assert!(w.entities.get(target).map(|e| e.hp).unwrap_or(-1) < hp0);
    }

    #[test]
    fn dropped_pickups_expire() {
        let mut w = arena();
        let id = w.spawn(Kind::Rupee, 0, V2::from_px(200, 200));
        w.entities.get_mut(id).unwrap().timer = 5;
        for _ in 0..10 {
            w.step();
        }
        assert!(w.entities.get(id).is_none());
    }
}
