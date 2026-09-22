//! Resolving who hit whom this frame.
//!
//! Collision pairs are gathered first and applied afterwards so that the order
//! entities happen to sit in the slab cannot change the outcome.

use crate::entity::{eflag, EntityId, Kind};
use crate::event::Sfx;
use crate::geom::{Dir, V2};
use crate::input::button;
use crate::items::Item;
use crate::world::{pstate, World};

/// Runs every damage and pickup interaction for the frame.
pub(crate) fn resolve(world: &mut World) {
    weapons_vs_monsters(world);
    monsters_vs_players(world);
    pickups(world);
}

/// Player weapons hitting anything that can be hurt.
fn weapons_vs_monsters(world: &mut World) {
    let mut hits: Vec<(EntityId, EntityId, i16, i32, bool)> = Vec::new();
    for (wid, weapon) in world.entities.iter() {
        let damage = match weapon.kind {
            Kind::SwordSwing => weapon.data[0].max(1) as i16,
            Kind::SwordBeam => 1,
            Kind::Arrow if weapon.has(eflag::PLAYER_OWNED) => 2,
            Kind::Boomerang => 0,
            _ => continue,
        };
        let stun_only = weapon.kind == Kind::Boomerang;
        let area = weapon.hurt_box();
        for (tid, target) in world.entities.iter() {
            if tid == wid || target.level != weapon.level {
                continue;
            }
            if !target.has(eflag::VULNERABLE) || target.kind == Kind::Player {
                continue;
            }
            // A weapon never hits the hand that holds it.
            if tid == weapon.owner {
                continue;
            }
            if target.hurt_box().overlaps(&area) {
                hits.push((tid, wid, damage, weapon.pos.x, stun_only));
            }
        }
    }

    for (target, weapon_id, damage, _x, stun_only) in hits {
        let Some(w) = world.entities.get(weapon_id) else {
            continue;
        };
        let from = w.pos;
        let kind = w.kind;
        let owner = w.owner;
        if stun_only {
            if let Some(t) = world.entities.get_mut(target) {
                t.stun = t.stun.max(45);
            }
            world.events.sound(Sfx::EnemyHit);
            continue;
        }
        let was_alive = world.entities.get(target).is_some();
        let landed = world.damage(target, damage, from, 6);
        if landed && was_alive && world.entities.get(target).is_none() {
            // Credit the kill to whoever owns the weapon.
            if let Some(o) = world.entities.get(owner) {
                let pi = o.player as usize;
                if o.kind == Kind::Player && pi < world.players.len() {
                    world.players[pi].kills += 1;
                }
            }
        }
        // Arrows and beams are spent on impact.
        if landed && matches!(kind, Kind::Arrow | Kind::SwordBeam) {
            world.entities.despawn(weapon_id);
        }
    }
}

/// Monsters and their projectiles hurting heroes.
fn monsters_vs_players(world: &mut World) {
    let mut hits: Vec<(usize, i16, V2, EntityId)> = Vec::new();
    for (pid, p) in world.entities.iter() {
        if p.kind != Kind::Player || p.iframes > 0 || p.state == pstate::DEAD {
            continue;
        }
        let pi = p.player as usize;
        if pi >= world.players.len() || !world.players[pi].is_alive() {
            continue;
        }
        let body = p.body();
        for (aid, a) in world.entities.iter() {
            if aid == pid || a.level != p.level || !a.has(eflag::TOUCH_HURTS) {
                continue;
            }
            // A hero's own arrow does not come back to bite them.
            if a.has(eflag::PLAYER_OWNED) || a.owner == pid {
                continue;
            }
            let damage = crate::entity::info(a.kind).touch_damage;
            if damage > 0 && a.hurt_box().overlaps(&body) {
                hits.push((pi, damage, a.pos, aid));
            }
        }
    }

    for (pi, damage, from, attacker) in hits {
        if shield_blocks(world, pi, from) {
            world.events.sound(Sfx::Shield);
            if let Some(a) = world.entities.get(attacker) {
                if a.kind.is_projectile() {
                    world.entities.despawn(attacker);
                }
            }
            continue;
        }
        if world.hurt_player(pi, damage, from) {
            if let Some(a) = world.entities.get(attacker) {
                if a.kind.is_projectile() {
                    world.entities.despawn(attacker);
                }
            }
        }
    }
}

/// True when the player is holding a shield towards the attack.
fn shield_blocks(world: &World, pi: usize, from: V2) -> bool {
    let Some(p) = world.players.get(pi) else {
        return false;
    };
    let Some(e) = world.entities.get(p.entity) else {
        return false;
    };
    if e.state == pstate::HURT {
        return false;
    }
    let slot = p.inv.equipped.iter().position(|i| *i == Item::Shield);
    let Some(slot) = slot else { return false };
    let held = if slot == 0 {
        p.input.held(button::A)
    } else {
        p.input.held(button::B)
    };
    if !held {
        return false;
    }
    let d = from.sub(e.pos);
    Dir::from_delta(d.x, d.y) == e.dir
}

/// Heroes walking over things worth picking up.
fn pickups(world: &mut World) {
    let mut taken: Vec<(usize, EntityId, Kind)> = Vec::new();
    for (pid, p) in world.entities.iter() {
        if p.kind != Kind::Player {
            continue;
        }
        let pi = p.player as usize;
        if pi >= world.players.len() || !world.players[pi].is_alive() {
            continue;
        }
        let reach = p.body().inflate(3);
        for (iid, item) in world.entities.iter() {
            if iid == pid || item.level != p.level || !item.kind.is_pickup() {
                continue;
            }
            if item.body().overlaps(&reach) {
                taken.push((pi, iid, item.kind));
            }
        }
    }
    // A pickup can only be claimed once even if two heroes reach it together.
    let mut claimed: Vec<EntityId> = Vec::new();
    for (pi, id, kind) in taken {
        if claimed.contains(&id) || world.entities.get(id).is_none() {
            continue;
        }
        claimed.push(id);
        world.entities.despawn(id);
        world.collect(pi, kind);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::px;
    use crate::level::{Level, LevelKind};
    use crate::tiles::tile;

    fn arena() -> World {
        let mut lv = Level::new(LevelKind::Overworld, 2, 2, tile::GRASS);
        lv.entrance = V2::from_px(80, 64);
        let mut w = World::new(11, vec![lv], 2);
        w.join(0);
        w
    }

    #[test]
    fn a_sword_swing_hurts_a_monster_in_front() {
        let mut w = arena();
        let pos = w.player_entity(0).unwrap().pos;
        let mob = w.spawn(Kind::Moblin, 0, V2::new(pos.x + px(14), pos.y));
        let hp0 = w.entities.get(mob).unwrap().hp;
        w.set_input(0, button::RIGHT);
        w.step();
        w.set_input(0, button::RIGHT | button::A);
        w.step();
        w.step();
        assert!(w.entities.get(mob).map(|e| e.hp).unwrap_or(-9) < hp0);
    }

    #[test]
    fn a_sword_swing_misses_behind_you() {
        let mut w = arena();
        let pos = w.player_entity(0).unwrap().pos;
        let mob = w.spawn(Kind::Moblin, 0, V2::new(pos.x - px(20), pos.y));
        let hp0 = w.entities.get(mob).unwrap().hp;
        w.set_input(0, button::RIGHT);
        w.step();
        w.set_input(0, button::RIGHT | button::A);
        for _ in 0..4 {
            w.step();
        }
        assert_eq!(w.entities.get(mob).unwrap().hp, hp0);
    }

    #[test]
    fn a_monster_hurts_a_hero_it_touches() {
        let mut w = arena();
        let pos = w.player_entity(0).unwrap().pos;
        w.spawn(Kind::Octorok, 0, pos);
        w.step();
        assert!(w.player_entity(0).unwrap().hp < 12);
    }

    #[test]
    fn a_raised_shield_stops_a_hit() {
        let mut w = arena();
        let pos = w.player_entity(0).unwrap().pos;
        w.players[0].inv.give(Item::Shield);
        w.players[0].inv.equipped[1] = Item::Shield;
        // A rock flying in from the right, straight at a raised shield.
        let rock = w.spawn(Kind::Rock, 0, V2::new(pos.x + px(20), pos.y));
        {
            let r = w.entities.get_mut(rock).unwrap();
            r.vel = V2::new(-px(2), 0);
            r.timer = 60;
        }
        w.set_input(0, button::RIGHT | button::B);
        for _ in 0..12 {
            w.step();
            if w.entities.get(rock).is_none() {
                break;
            }
        }
        assert!(w.entities.get(rock).is_none(), "the rock should be spent");
        assert_eq!(w.player_entity(0).unwrap().hp, 12, "the shield should hold");
        assert!(w.events.has_sound(Sfx::Shield));
    }

    #[test]
    fn an_unshielded_hero_takes_that_same_rock() {
        let mut w = arena();
        let pos = w.player_entity(0).unwrap().pos;
        let rock = w.spawn(Kind::Rock, 0, V2::new(pos.x + px(20), pos.y));
        {
            let r = w.entities.get_mut(rock).unwrap();
            r.vel = V2::new(-px(2), 0);
            r.timer = 60;
        }
        for _ in 0..12 {
            w.set_input(0, button::RIGHT);
            w.step();
        }
        assert!(w.player_entity(0).unwrap().hp < 12);
    }

    #[test]
    fn hearts_are_picked_up_and_heal() {
        let mut w = arena();
        w.hurt_player(0, 4, V2::from_px(0, 0));
        let hp0 = w.player_entity(0).unwrap().hp;
        let pos = w.player_entity(0).unwrap().pos;
        w.spawn(Kind::Heart, 0, pos);
        w.step();
        assert!(w.player_entity(0).unwrap().hp > hp0);
        assert!(w.events.has_sound(Sfx::Heart));
    }

    #[test]
    fn one_rupee_cannot_be_taken_by_two_heroes() {
        let mut w = arena();
        w.join(1);
        let pos = w.player_entity(0).unwrap().pos;
        if let Some(e) = w.entities.get_mut(w.players[1].entity) {
            e.pos = pos;
        }
        w.spawn(Kind::Rupee, 0, pos);
        w.step();
        let total = w.players[0].inv.rupees + w.players[1].inv.rupees;
        assert_eq!(total, 1, "the rupee must be counted exactly once");
    }

    #[test]
    fn a_hero_arrow_does_not_hurt_its_owner() {
        let mut w = arena();
        let pos = w.player_entity(0).unwrap().pos;
        let owner = w.players[0].entity;
        let id = w.spawn(Kind::Arrow, 0, pos);
        {
            let a = w.entities.get_mut(id).unwrap();
            a.owner = owner;
            a.flags |= eflag::PLAYER_OWNED;
            a.timer = 30;
        }
        w.step();
        assert_eq!(w.player_entity(0).unwrap().hp, 12);
    }

    #[test]
    fn killing_a_monster_credits_the_hero() {
        let mut w = arena();
        let pos = w.player_entity(0).unwrap().pos;
        let mob = w.spawn(Kind::Keese, 0, V2::new(pos.x + px(14), pos.y));
        w.entities.get_mut(mob).unwrap().hp = 1;
        w.set_input(0, button::RIGHT);
        w.step();
        w.set_input(0, button::RIGHT | button::A);
        for _ in 0..4 {
            w.step();
        }
        assert_eq!(w.players[0].kills, 1);
    }
}
