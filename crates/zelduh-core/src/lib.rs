//! Zelduh: a deterministic top-down action-adventure engine in the style of
//! the Game Boy Zelda games.
//!
//! The crate is split so that the parts that must be identical on every machine
//! in a multiplayer session stay free of anything platform specific:
//!
//! * [`fixed`], [`geom`], [`rng`] — integer maths, no floating point anywhere.
//! * [`level`], [`tiles`] — the map, in logical terms rather than pixels.
//! * [`entity`], [`player`], [`enemy`], [`objects`], [`combat`] — what moves.
//! * [`world`] — [`World::step`](world::World::step), one frame at a time.
//!
//! Rendering, asset loading and networking all live in other crates and read
//! this one; nothing here reads them.

pub mod boss;
pub mod combat;
pub mod entity;
pub mod event;
pub mod fixed;
pub mod geom;
pub mod input;
pub mod items;
pub mod level;
pub mod objects;
pub mod physics;
pub mod player;
pub mod rng;
pub mod save;
pub mod tiles;
pub mod world;

pub mod enemy;

pub use entity::{Entities, Entity, EntityId, Kind};
pub use event::{Event, Events, Sfx};
pub use geom::{Dir, Rect, V2};
pub use input::{button, Input};
pub use items::{Inventory, Item};
pub use level::{Level, LevelKind, Map, Room, RoomKind, Spawn};
pub use rng::Rng;
pub use world::{Camera, Player, Role, World};
