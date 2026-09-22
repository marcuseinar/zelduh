//! Choosing which image to draw for an entity.

use zelduh_assets::pack::SpriteId;
use zelduh_assets::tile::FLIP_X;
use zelduh_core::entity::{eflag, Entity, Kind};
use zelduh_core::geom::Dir;
use zelduh_core::items::Item;
use zelduh_core::world::{pstate, World};

/// The image and flip to draw for an entity, or `None` to draw nothing.
pub fn for_entity(world: &World, e: &Entity) -> Option<(SpriteId, u8)> {
    use SpriteId::*;
    let side_flip = if e.dir == Dir::Left { FLIP_X } else { 0 };
    Some(match e.kind {
        Kind::Player => return player(world, e),
        Kind::Octorok => (if e.anim & 1 == 0 { Octorok0 } else { Octorok1 }, side_flip),
        Kind::Moblin => (if e.anim & 1 == 0 { Moblin0 } else { Moblin1 }, side_flip),
        Kind::Zol => (if e.anim & 1 == 0 { Zol0 } else { Zol1 }, 0),
        Kind::Keese => (if e.anim & 1 == 0 { Keese0 } else { Keese1 }, 0),
        Kind::Tektite => (if e.anim & 1 == 0 { Tektite0 } else { Tektite1 }, 0),
        Kind::Stalfos => (if e.anim & 1 == 0 { Stalfos0 } else { Stalfos1 }, side_flip),
        Kind::Boss => (if (world.frame / 12) % 2 == 0 { Boss0 } else { Boss1 }, 0),
        Kind::Rock => (Rock, 0),
        Kind::Fireball => (Fireball, 0),
        Kind::Arrow => match e.dir {
            Dir::Up => (ArrowVert, 0),
            Dir::Down => (ArrowVert, zelduh_assets::tile::FLIP_Y),
            Dir::Left => (ArrowSide, FLIP_X),
            Dir::Right => (ArrowSide, 0),
        },
        Kind::SwordBeam => match e.dir {
            Dir::Up | Dir::Down => (BeamVert, 0),
            _ => (BeamSide, 0),
        },
        Kind::Boomerang => (Boomerang, if (world.frame / 3) % 2 == 0 { 0 } else { FLIP_X }),
        Kind::Bomb => (Bomb, 0),
        Kind::Explosion => (Explosion, 0),
        Kind::SwordSwing => match e.dir {
            Dir::Up => (SwordUp, 0),
            Dir::Down => (SwordUp, zelduh_assets::tile::FLIP_Y),
            Dir::Left => (SwordSide, FLIP_X),
            Dir::Right => (SwordSide, 0),
        },
        // A carried tile is drawn from its terrain art by the caller.
        Kind::Carried => return None,
        Kind::Rupee => (Rupee, 0),
        Kind::Heart => (Heart, 0),
        Kind::Key => (Key, 0),
        Kind::BombPickup => (BombPickup, 0),
        Kind::ArrowPickup => (ArrowPickup, 0),
        Kind::Fairy => (Fairy, if (world.frame / 6) % 2 == 0 { 0 } else { FLIP_X }),
        Kind::HeartPiece => (HeartPiece, 0),
        Kind::Triforce => (Triforce, 0),
        Kind::Chest => (if e.state == 0 { ChestClosed } else { ChestOpen }, 0),
        Kind::Poof => (Poof, 0),
        Kind::Sparkle => (Sparkle, 0),
        Kind::None => return None,
    })
}

/// The hero's pose.
fn player(world: &World, e: &Entity) -> Option<(SpriteId, u8)> {
    use SpriteId::*;
    if e.state == pstate::DEAD {
        return None;
    }
    let side_flip = if e.dir == Dir::Left { FLIP_X } else { 0 };
    let carrying = world
        .players
        .get(e.player as usize)
        .map(|p| !p.carrying.is_none())
        .unwrap_or(false);

    let id = match e.state {
        pstate::FALL => return Some((HeroFall, 0)),
        pstate::SWIM => return Some((HeroSwim, side_flip)),
        pstate::ATTACK => match e.dir {
            Dir::Down => HeroAttackDown,
            Dir::Up => HeroAttackUp,
            _ => HeroAttackSide,
        },
        _ if carrying => match e.dir {
            Dir::Down => HeroCarryDown,
            Dir::Up => HeroCarryUp,
            _ => HeroCarrySide,
        },
        _ => {
            // Frames 0 and 2 are the neutral pose, 1 and 3 the step.
            let stepping = e.anim & 1 == 1;
            match (e.dir, stepping) {
                (Dir::Down, false) => HeroDown0,
                (Dir::Down, true) => HeroDown1,
                (Dir::Up, false) => HeroUp0,
                (Dir::Up, true) => HeroUp1,
                (_, false) => HeroSide0,
                (_, true) => HeroSide1,
            }
        }
    };
    Some((id, side_flip))
}

/// The status bar icon for an item.
pub fn item_icon(item: Item) -> SpriteId {
    use SpriteId::*;
    match item {
        Item::Sword => IconSword,
        Item::Shield => IconShield,
        Item::Bombs => IconBomb,
        Item::Bow => IconBow,
        Item::Boomerang => IconBoomerang,
        Item::Feather => IconFeather,
        Item::Bracelet => IconBracelet,
        Item::Boots => IconBoots,
        Item::Flippers => IconFlippers,
        Item::None => Sparkle,
    }
}

/// True when this entity is drawn under everything else.
pub fn is_floor_layer(e: &Entity) -> bool {
    e.has(eflag::FLOOR_LAYER)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zelduh_core::level::{Level, LevelKind};
    use zelduh_core::tiles::tile;
    use zelduh_core::{V2, World};

    fn world() -> World {
        let mut lv = Level::new(LevelKind::Overworld, 2, 2, tile::GRASS);
        lv.entrance = V2::from_px(80, 64);
        let mut w = World::new(3, vec![lv], 1);
        w.join(0);
        w
    }

    #[test]
    fn every_live_kind_has_an_image() {
        let mut w = world();
        for kind in [
            Kind::Octorok,
            Kind::Moblin,
            Kind::Zol,
            Kind::Keese,
            Kind::Tektite,
            Kind::Stalfos,
            Kind::Boss,
            Kind::Rock,
            Kind::Arrow,
            Kind::Bomb,
            Kind::Rupee,
            Kind::Heart,
            Kind::Chest,
        ] {
            let id = w.spawn(kind, 0, V2::from_px(40, 40));
            let e = w.entities.get(id).unwrap();
            assert!(for_entity(&w, e).is_some(), "{kind:?} has no sprite");
        }
    }

    #[test]
    fn the_hero_faces_left_by_mirroring() {
        let mut w = world();
        let eid = w.players[0].entity;
        w.entities.get_mut(eid).unwrap().dir = Dir::Left;
        let e = w.entities.get(eid).unwrap();
        let (id, flip) = for_entity(&w, e).unwrap();
        assert_eq!(id, SpriteId::HeroSide0);
        assert_eq!(flip, FLIP_X);
    }

    #[test]
    fn swimming_and_attacking_change_the_pose() {
        let mut w = world();
        let eid = w.players[0].entity;
        w.entities.get_mut(eid).unwrap().state = pstate::SWIM;
        let e = w.entities.get(eid).unwrap();
        assert_eq!(for_entity(&w, e).unwrap().0, SpriteId::HeroSwim);
        w.entities.get_mut(eid).unwrap().state = pstate::ATTACK;
        let e = w.entities.get(eid).unwrap();
        assert_eq!(for_entity(&w, e).unwrap().0, SpriteId::HeroAttackDown);
    }

    #[test]
    fn a_dead_hero_is_not_drawn() {
        let mut w = world();
        let eid = w.players[0].entity;
        w.entities.get_mut(eid).unwrap().state = pstate::DEAD;
        let e = w.entities.get(eid).unwrap();
        assert!(for_entity(&w, e).is_none());
    }
}
