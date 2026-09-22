//! Events the simulation emits for presentation layers.
//!
//! The simulation never plays a sound or draws anything itself; it pushes an
//! event and lets the renderer and audio code decide what that means. Events
//! are cleared at the start of every step.

use crate::geom::V2;

/// A sound effect id.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Sfx {
    SwordSwing = 0,
    SwordBeam = 1,
    EnemyHit = 2,
    EnemyDie = 3,
    PlayerHurt = 4,
    PlayerDie = 5,
    Pickup = 6,
    Rupee = 7,
    Heart = 8,
    BombPlace = 9,
    Explosion = 10,
    Shoot = 11,
    Boomerang = 12,
    DoorOpen = 13,
    Unlock = 14,
    ChestOpen = 15,
    Secret = 16,
    Jump = 17,
    Splash = 18,
    Fall = 19,
    Lift = 20,
    Throw = 21,
    Shield = 22,
    Text = 23,
    BossHurt = 24,
    BossDie = 25,
    Stairs = 26,
    Error = 27,
}

/// Something that happened this frame.
#[derive(Clone, Debug)]
pub enum Event {
    /// Play a sound.
    Sound(Sfx),
    /// A hit landed at a position, for sparks and screen shake.
    Hit { at: V2, heavy: bool },
    /// A player took damage.
    PlayerHurt { player: u8, amount: i16 },
    /// A player died.
    PlayerDied { player: u8 },
    /// A player moved to another level.
    LevelChanged { player: u8, level: u16 },
    /// A player entered a new room.
    RoomChanged { player: u8, rx: i32, ry: i32 },
    /// A message to display in a text box.
    Message(&'static str),
    /// A boss was defeated on a level.
    BossDefeated { level: u16 },
    /// Shake the screen for a number of frames.
    Shake { frames: u8 },
}

/// A small queue of events, reused between frames.
#[derive(Clone, Debug, Default)]
pub struct Events {
    items: Vec<Event>,
}

impl Events {
    pub fn new() -> Events {
        Events::default()
    }

    pub fn push(&mut self, e: Event) {
        // A runaway producer must never grow this without bound.
        if self.items.len() < 256 {
            self.items.push(e);
        }
    }

    pub fn sound(&mut self, s: Sfx) {
        self.push(Event::Sound(s));
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = &Event> {
        self.items.iter()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// True when a given sound was emitted this frame. Handy in tests.
    pub fn has_sound(&self, s: Sfx) -> bool {
        self.items
            .iter()
            .any(|e| matches!(e, Event::Sound(x) if *x == s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_is_bounded() {
        let mut q = Events::new();
        for _ in 0..1000 {
            q.sound(Sfx::EnemyHit);
        }
        assert_eq!(q.len(), 256);
    }

    #[test]
    fn has_sound_finds_it() {
        let mut q = Events::new();
        q.sound(Sfx::Rupee);
        assert!(q.has_sound(Sfx::Rupee));
        assert!(!q.has_sound(Sfx::Explosion));
        q.clear();
        assert!(q.is_empty());
    }
}
