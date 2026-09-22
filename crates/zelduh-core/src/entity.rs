//! Entities and the slab they live in.
//!
//! Everything that is not terrain is an entity: players, enemies, the rock an
//! octorok spits, a lifted bush in mid-air, a heart on the floor. They live in
//! a slab with generation counters so a stale [`EntityId`] can never be
//! mistaken for a live one.

use crate::fixed::{px, Fx};
use crate::geom::{Dir, Rect, V2};

/// What an entity is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum Kind {
    #[default]
    None = 0,
    Player = 1,

    // Enemies.
    Octorok = 16,
    Moblin = 17,
    Zol = 18,
    Keese = 19,
    Tektite = 20,
    Stalfos = 21,
    /// A dungeon boss. `data[0]` selects which one.
    Boss = 31,

    // Projectiles and weapons.
    Rock = 48,
    Arrow = 49,
    SwordBeam = 50,
    Boomerang = 51,
    Bomb = 52,
    Explosion = 53,
    /// The sword blade itself while a swing is in progress.
    SwordSwing = 54,
    Fireball = 55,

    // Pickups.
    Rupee = 64,
    Heart = 65,
    Key = 66,
    BombPickup = 67,
    ArrowPickup = 68,
    Fairy = 69,
    HeartPiece = 70,
    Triforce = 71,

    // World objects.
    Chest = 80,
    /// A tile the player has picked up and may throw. `data[0]` is the tile id.
    Carried = 81,
    Poof = 82,
    Sparkle = 83,
}

impl Kind {
    pub fn from_u8(v: u8) -> Kind {
        use Kind::*;
        match v {
            1 => Player,
            16 => Octorok,
            17 => Moblin,
            18 => Zol,
            19 => Keese,
            20 => Tektite,
            21 => Stalfos,
            31 => Boss,
            48 => Rock,
            49 => Arrow,
            50 => SwordBeam,
            51 => Boomerang,
            52 => Bomb,
            53 => Explosion,
            54 => SwordSwing,
            55 => Fireball,
            64 => Rupee,
            65 => Heart,
            66 => Key,
            67 => BombPickup,
            68 => ArrowPickup,
            69 => Fairy,
            70 => HeartPiece,
            71 => Triforce,
            80 => Chest,
            81 => Carried,
            82 => Poof,
            83 => Sparkle,
            _ => None,
        }
    }

    /// True for anything that fights on the monsters' side.
    pub fn is_enemy(self) -> bool {
        use Kind::*;
        matches!(
            self,
            Octorok | Moblin | Zol | Keese | Tektite | Stalfos | Boss
        )
    }

    /// True for anything the player walks over to collect.
    pub fn is_pickup(self) -> bool {
        use Kind::*;
        matches!(
            self,
            Rupee | Heart | Key | BombPickup | ArrowPickup | Fairy | HeartPiece | Triforce
        )
    }

    /// True for anything that flies in a straight-ish line and dies on impact.
    pub fn is_projectile(self) -> bool {
        use Kind::*;
        matches!(self, Rock | Arrow | SwordBeam | Boomerang | Fireball)
    }
}

/// Per-entity behaviour bits.
pub mod eflag {
    /// Takes damage from player weapons.
    pub const VULNERABLE: u32 = 1 << 0;
    /// Hurts players on contact.
    pub const TOUCH_HURTS: u32 = 1 << 1;
    /// Ignores terrain collision.
    pub const GHOST: u32 = 1 << 2;
    /// Flies: passes over pits and water, blocked only by tall terrain.
    pub const FLYING: u32 = 1 << 3;
    /// Pushed around by other bodies.
    pub const SOLID_BODY: u32 = 1 << 4;
    /// Removed when its room is far from every player.
    pub const DESPAWN_FAR: u32 = 1 << 5;
    /// Dies when it hits terrain.
    pub const DIE_ON_WALL: u32 = 1 << 6;
    /// Belongs to a player, so it should not hurt players.
    pub const PLAYER_OWNED: u32 = 1 << 7;
    /// Drawn behind everything else (shadows, floor effects).
    pub const FLOOR_LAYER: u32 = 1 << 8;
    /// Currently airborne; `z` is above zero.
    pub const AIRBORNE: u32 = 1 << 9;
    /// A boss: bigger, immune to knockback, drives its own room state.
    pub const BOSS: u32 = 1 << 10;
    /// Under the control of a human player rather than the AI.
    pub const POSSESSED: u32 = 1 << 11;
}

/// Static per-kind properties.
#[derive(Clone, Copy, Debug)]
pub struct KindInfo {
    /// Collision body width in pixels.
    pub body_w: i32,
    /// Collision body height in pixels.
    pub body_h: i32,
    /// Starting health, in the same units as player quarter-hearts for players
    /// and plain hit points for monsters.
    pub hp: i16,
    /// Contact damage dealt to a player, in quarter hearts.
    pub touch_damage: i16,
    /// Base movement speed in fixed-point pixels per frame.
    pub speed: Fx,
    pub flags: u32,
}

impl Default for KindInfo {
    fn default() -> Self {
        KindInfo {
            body_w: 12,
            body_h: 12,
            hp: 1,
            touch_damage: 0,
            speed: 0,
            flags: 0,
        }
    }
}

/// Static properties for a kind.
pub fn info(kind: Kind) -> KindInfo {
    use eflag::*;
    use Kind::*;
    let d = KindInfo::default;
    match kind {
        Player => KindInfo {
            body_w: 10,
            body_h: 8,
            hp: 12,
            speed: 320,
            flags: SOLID_BODY,
            ..d()
        },
        Octorok => KindInfo {
            body_w: 12,
            body_h: 12,
            hp: 2,
            touch_damage: 2,
            speed: 96,
            flags: VULNERABLE | TOUCH_HURTS | SOLID_BODY | DESPAWN_FAR,
        },
        Moblin => KindInfo {
            body_w: 13,
            body_h: 13,
            hp: 3,
            touch_damage: 2,
            speed: 128,
            flags: VULNERABLE | TOUCH_HURTS | SOLID_BODY | DESPAWN_FAR,
        },
        Zol => KindInfo {
            body_w: 12,
            body_h: 10,
            hp: 2,
            touch_damage: 2,
            speed: 64,
            flags: VULNERABLE | TOUCH_HURTS | SOLID_BODY | DESPAWN_FAR,
        },
        Keese => KindInfo {
            body_w: 10,
            body_h: 10,
            hp: 1,
            touch_damage: 2,
            speed: 192,
            flags: VULNERABLE | TOUCH_HURTS | FLYING | DESPAWN_FAR,
        },
        Tektite => KindInfo {
            body_w: 12,
            body_h: 12,
            hp: 2,
            touch_damage: 2,
            speed: 224,
            flags: VULNERABLE | TOUCH_HURTS | SOLID_BODY | DESPAWN_FAR,
        },
        Stalfos => KindInfo {
            body_w: 12,
            body_h: 14,
            hp: 4,
            touch_damage: 4,
            speed: 144,
            flags: VULNERABLE | TOUCH_HURTS | SOLID_BODY | DESPAWN_FAR,
        },
        Boss => KindInfo {
            body_w: 28,
            body_h: 26,
            hp: 24,
            touch_damage: 4,
            speed: 128,
            flags: VULNERABLE | TOUCH_HURTS | SOLID_BODY | BOSS,
        },
        Rock | Fireball => KindInfo {
            body_w: 6,
            body_h: 6,
            touch_damage: 2,
            speed: 288,
            flags: FLYING | DIE_ON_WALL | TOUCH_HURTS,
            ..d()
        },
        Arrow => KindInfo {
            body_w: 6,
            body_h: 6,
            touch_damage: 2,
            speed: 448,
            flags: FLYING | DIE_ON_WALL,
            ..d()
        },
        SwordBeam => KindInfo {
            body_w: 8,
            body_h: 8,
            speed: 448,
            flags: FLYING | DIE_ON_WALL | PLAYER_OWNED,
            ..d()
        },
        Boomerang => KindInfo {
            body_w: 8,
            body_h: 8,
            speed: 352,
            flags: FLYING | PLAYER_OWNED,
            ..d()
        },
        Bomb => KindInfo {
            body_w: 10,
            body_h: 10,
            flags: 0,
            ..d()
        },
        Explosion => KindInfo {
            body_w: 28,
            body_h: 28,
            flags: GHOST,
            ..d()
        },
        SwordSwing => KindInfo {
            body_w: 12,
            body_h: 12,
            flags: GHOST | PLAYER_OWNED,
            ..d()
        },
        Carried => KindInfo {
            body_w: 12,
            body_h: 12,
            flags: GHOST,
            ..d()
        },
        Chest => KindInfo {
            body_w: 16,
            body_h: 14,
            flags: 0,
            ..d()
        },
        Poof | Sparkle => KindInfo {
            body_w: 8,
            body_h: 8,
            flags: GHOST,
            ..d()
        },
        _ if kind.is_pickup() => KindInfo {
            body_w: 10,
            body_h: 10,
            flags: GHOST,
            ..d()
        },
        _ => d(),
    }
}

/// A handle to an entity that goes stale when the slot is reused.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct EntityId {
    pub idx: u16,
    pub gen: u16,
}

impl EntityId {
    pub const NONE: EntityId = EntityId {
        idx: u16::MAX,
        gen: 0,
    };

    pub fn is_none(self) -> bool {
        self.idx == u16::MAX
    }
}

/// A live thing in the world.
#[derive(Clone, Debug)]
pub struct Entity {
    pub kind: Kind,
    /// Index of the level this entity is on.
    pub level: u16,
    /// Centre of the collision body, in world pixels.
    pub pos: V2,
    pub vel: V2,
    /// Height above the floor, for jumps and thrown objects.
    pub z: Fx,
    pub vz: Fx,
    pub dir: Dir,
    pub hp: i16,
    pub max_hp: i16,
    /// Knockback applied over the next few frames.
    pub knock: V2,
    pub knock_frames: u8,
    /// Frames of damage immunity remaining.
    pub iframes: u8,
    /// Frames the entity cannot act for.
    pub stun: u8,
    /// Behaviour state, interpreted per kind.
    pub state: u8,
    /// Frames remaining in the current state.
    pub timer: u16,
    pub anim: u8,
    pub anim_timer: u8,
    pub flags: u32,
    /// Scratch space, interpreted per kind.
    pub data: [i32; 4],
    /// The entity that created this one, for projectiles and swings.
    pub owner: EntityId,
    /// Index into [`crate::world::World::players`] for player entities.
    pub player: u8,
    pub body_w: i32,
    pub body_h: i32,
    pub alive: bool,
    gen: u16,
}

impl Default for Entity {
    fn default() -> Self {
        Entity {
            kind: Kind::None,
            level: 0,
            pos: V2::ZERO,
            vel: V2::ZERO,
            z: 0,
            vz: 0,
            dir: Dir::Down,
            hp: 1,
            max_hp: 1,
            knock: V2::ZERO,
            knock_frames: 0,
            iframes: 0,
            stun: 0,
            state: 0,
            timer: 0,
            anim: 0,
            anim_timer: 0,
            flags: 0,
            data: [0; 4],
            owner: EntityId::NONE,
            player: u8::MAX,
            body_w: 12,
            body_h: 12,
            alive: false,
            gen: 0,
        }
    }
}

impl Entity {
    /// Collision body in world space.
    pub fn body(&self) -> Rect {
        Rect::centered(self.pos, self.body_w, self.body_h)
    }

    /// Body used for taking damage: a bit taller than the feet box so that
    /// slashing at the top of a sprite connects.
    pub fn hurt_box(&self) -> Rect {
        let h = self.body_h.max(12) + 4;
        Rect::new(
            self.pos.x - px(self.body_w) / 2,
            self.pos.y - px(h) + px(self.body_h) / 2,
            px(self.body_w),
            px(h),
        )
    }

    #[inline]
    pub fn has(&self, f: u32) -> bool {
        self.flags & f != 0
    }

    #[inline]
    pub fn set_flag(&mut self, f: u32, on: bool) {
        if on {
            self.flags |= f;
        } else {
            self.flags &= !f;
        }
    }

    /// The generation counter for this slot, used by snapshots.
    pub fn generation(&self) -> u16 {
        self.gen
    }

    /// Restores a generation counter when loading a snapshot.
    pub fn set_generation(&mut self, gen: u16) {
        self.gen = gen;
    }

    /// True when the entity can be hurt right now.
    pub fn can_be_hurt(&self) -> bool {
        self.alive && self.has(eflag::VULNERABLE) && self.iframes == 0 && self.hp > 0
    }
}

/// The slab of entities.
#[derive(Clone, Debug, Default)]
pub struct Entities {
    slots: Vec<Entity>,
    free: Vec<u16>,
    live: usize,
}

impl Entities {
    pub fn new() -> Entities {
        Entities::default()
    }

    /// Number of live entities.
    pub fn len(&self) -> usize {
        self.live
    }

    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// Number of slots, live or not. Iterating `0..capacity()` covers them all.
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// Spawns an entity of `kind` at `pos` on `level`, with the kind's defaults.
    pub fn spawn(&mut self, kind: Kind, level: u16, pos: V2) -> EntityId {
        let ki = info(kind);
        let idx = match self.free.pop() {
            Some(i) => i,
            None => {
                self.slots.push(Entity::default());
                (self.slots.len() - 1) as u16
            }
        };
        let gen = self.slots[idx as usize].gen.wrapping_add(1).max(1);
        let e = &mut self.slots[idx as usize];
        *e = Entity {
            kind,
            level,
            pos,
            hp: ki.hp,
            max_hp: ki.hp,
            flags: ki.flags,
            body_w: ki.body_w,
            body_h: ki.body_h,
            alive: true,
            gen,
            ..Entity::default()
        };
        self.live += 1;
        EntityId { idx, gen }
    }

    /// Marks an entity dead and frees its slot.
    pub fn despawn(&mut self, id: EntityId) {
        if let Some(e) = self.get_mut(id) {
            e.alive = false;
            self.free.push(id.idx);
            self.live -= 1;
        }
    }

    /// Frees every slot whose entity died during the step.
    pub fn collect_dead(&mut self) {
        for i in 0..self.slots.len() {
            let e = &self.slots[i];
            if !e.alive && e.kind != Kind::None {
                self.slots[i].kind = Kind::None;
                if !self.free.contains(&(i as u16)) {
                    self.free.push(i as u16);
                    self.live = self.live.saturating_sub(1);
                }
            }
        }
    }

    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        self.slots
            .get(id.idx as usize)
            .filter(|e| e.alive && e.gen == id.gen)
    }

    pub fn get_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        self.slots
            .get_mut(id.idx as usize)
            .filter(|e| e.alive && e.gen == id.gen)
    }

    /// Entity in a slot regardless of generation, for iteration by index.
    pub fn at(&self, idx: usize) -> Option<&Entity> {
        self.slots.get(idx).filter(|e| e.alive)
    }

    pub fn at_mut(&mut self, idx: usize) -> Option<&mut Entity> {
        self.slots.get_mut(idx).filter(|e| e.alive)
    }

    /// Handle for a slot index.
    pub fn id_of(&self, idx: usize) -> EntityId {
        match self.slots.get(idx) {
            Some(e) => EntityId {
                idx: idx as u16,
                gen: e.gen,
            },
            None => EntityId::NONE,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (EntityId, &Entity)> {
        self.slots.iter().enumerate().filter_map(|(i, e)| {
            if e.alive {
                Some((
                    EntityId {
                        idx: i as u16,
                        gen: e.gen,
                    },
                    e,
                ))
            } else {
                None
            }
        })
    }

    /// Every slot, live or not, in index order. Snapshots need the dead ones
    /// too so that handles keep pointing at the same slots after a restore.
    pub fn slots(&self) -> &[Entity] {
        &self.slots
    }

    /// The free list, in the order slots will be handed out again.
    ///
    /// Snapshots have to carry this: which slot the next spawn lands in decides
    /// the order entities are visited in, and therefore what the simulation
    /// does next. Rebuilding it in index order is not the same slab.
    pub fn free_slots(&self) -> &[u16] {
        &self.free
    }

    /// Rebuilds a slab from slots and a free list read out of a snapshot.
    pub fn rebuild(slots: Vec<Entity>, free: Vec<u16>) -> Entities {
        let live = slots.iter().filter(|e| e.alive).count();
        let free = free
            .into_iter()
            .filter(|i| slots.get(*i as usize).map(|e| !e.alive).unwrap_or(false))
            .collect();
        Entities { slots, free, live }
    }

    /// Removes every entity, keeping allocated storage.
    pub fn clear(&mut self) {
        self.slots.clear();
        self.free.clear();
        self.live = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_handles_do_not_resolve() {
        let mut es = Entities::new();
        let a = es.spawn(Kind::Octorok, 0, V2::from_px(10, 10));
        es.despawn(a);
        es.collect_dead();
        let b = es.spawn(Kind::Moblin, 0, V2::from_px(20, 20));
        assert_eq!(a.idx, b.idx, "the slot should be reused");
        assert!(es.get(a).is_none(), "the old handle must not resolve");
        assert!(es.get(b).is_some());
    }

    #[test]
    fn spawn_applies_kind_defaults() {
        let mut es = Entities::new();
        let id = es.spawn(Kind::Moblin, 0, V2::from_px(5, 5));
        let e = es.get(id).unwrap();
        assert_eq!(e.hp, info(Kind::Moblin).hp);
        assert!(e.has(eflag::VULNERABLE));
        assert!(e.kind.is_enemy());
    }

    #[test]
    fn len_tracks_live_entities() {
        let mut es = Entities::new();
        assert!(es.is_empty());
        let a = es.spawn(Kind::Rupee, 0, V2::ZERO);
        let _b = es.spawn(Kind::Heart, 0, V2::ZERO);
        assert_eq!(es.len(), 2);
        es.despawn(a);
        assert_eq!(es.len(), 1);
        es.collect_dead();
        assert_eq!(es.len(), 1);
    }

    #[test]
    fn kind_roundtrips_through_u8() {
        for k in [Kind::Player, Kind::Octorok, Kind::Boss, Kind::Chest] {
            assert_eq!(Kind::from_u8(k as u8), k);
        }
    }

    #[test]
    fn hurt_box_sits_above_the_feet() {
        let mut es = Entities::new();
        let id = es.spawn(Kind::Octorok, 0, V2::from_px(100, 100));
        let e = es.get(id).unwrap();
        assert!(e.hurt_box().y < e.body().y);
        assert!(e.hurt_box().bottom() >= e.body().bottom() - px(1));
    }
}
