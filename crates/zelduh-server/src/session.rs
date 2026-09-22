//! The shared game session.
//!
//! The simulation is deterministic, so the server does not need to send state
//! sixty times a second: it sends *inputs*. Every client steps its own copy of
//! the world from the same input stream and arrives at the same place. The
//! server keeps a copy of its own for two reasons: to hand a snapshot to
//! anyone joining a game already in progress, and to have something to compare
//! checksums against when a client drifts.
//!
//! Anything other than input that changes the world -- a player joining,
//! leaving, or taking over a boss -- has to happen on an agreed frame, so those
//! are carried in the same per-frame message rather than applied whenever they
//! happen to arrive.

use std::collections::VecDeque;
use std::sync::mpsc::Sender;

use zelduh_core::World;
use zelduh_gen::Config;

/// How many players one session holds.
pub const MAX_PLAYERS: usize = 4;
/// Simulation rate.
pub const TICK_HZ: u64 = 60;
/// How many past checksums to remember when looking for divergence.
const CHECK_HISTORY: usize = 600;
/// Size of the fixed part of a welcome message, before the snapshot: the
/// message id, slot, player count, seed, frame and snapshot length.
pub const WELCOME_HEADER: usize = 1 + 1 + 1 + 8 + 4 + 4;

/// Messages the server sends.
pub mod s2c {
    pub const WELCOME: u8 = 1;
    pub const FRAME: u8 = 2;
    pub const DESYNC: u8 = 3;
    pub const INFO: u8 = 4;
}

/// Messages a client sends.
pub mod c2s {
    pub const INPUT: u8 = 0x10;
    pub const CHECKSUM: u8 = 0x11;
    pub const ROLE: u8 = 0x12;
}

/// What a player is playing as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Hero,
    Boss,
}

#[derive(Clone, Default)]
struct Slot {
    occupied: bool,
    buttons: u16,
    /// Set when this slot should join the world on the next tick.
    pending_join: bool,
    /// Set when this slot should take over a boss on the next tick.
    pending_boss: bool,
    /// Set when this slot should leave the world on the next tick.
    pending_leave: bool,
    out: Option<Sender<Vec<u8>>>,
}

/// One running game.
pub struct Session {
    pub seed: u64,
    pub world: World,
    pub frame: u32,
    slots: Vec<Slot>,
    checks: VecDeque<(u32, u64)>,
    /// Counts of desyncs seen, for the log.
    pub desyncs: u32,
}

impl Session {
    /// Starts a fresh session with a generated world.
    pub fn new(seed: u64) -> Session {
        Session {
            seed,
            world: zelduh_gen::new_world(Config::from_seed(seed), MAX_PLAYERS),
            frame: 0,
            slots: vec![Slot::default(); MAX_PLAYERS],
            checks: VecDeque::new(),
            desyncs: 0,
        }
    }

    /// Number of players currently connected.
    pub fn player_count(&self) -> usize {
        self.slots.iter().filter(|s| s.occupied).count()
    }

    /// A bit per occupied slot.
    pub fn occupancy(&self) -> u8 {
        self.slots.iter().enumerate().fold(
            0u8,
            |acc, (i, s)| if s.occupied { acc | 1 << i } else { acc },
        )
    }

    /// Takes the next free slot for a new connection.
    ///
    /// Returns the slot and the welcome message, which carries the seed, the
    /// frame the snapshot was taken at, and the snapshot itself when the game
    /// is already under way.
    pub fn join(&mut self, out: Sender<Vec<u8>>, role: Role) -> Option<(usize, Vec<u8>)> {
        let slot = self.slots.iter().position(|s| !s.occupied)?;
        self.slots[slot] = Slot {
            occupied: true,
            buttons: 0,
            pending_join: role == Role::Hero,
            pending_boss: role == Role::Boss,
            pending_leave: false,
            out: Some(out),
        };

        let snapshot = if self.frame == 0 {
            // Nothing has happened yet, so the seed alone says everything.
            Vec::new()
        } else {
            self.world.save()
        };

        let mut msg = Vec::with_capacity(WELCOME_HEADER + snapshot.len());
        msg.push(s2c::WELCOME);
        msg.push(slot as u8);
        msg.push(MAX_PLAYERS as u8);
        msg.extend_from_slice(&self.seed.to_le_bytes());
        msg.extend_from_slice(&self.frame.to_le_bytes());
        msg.extend_from_slice(&(snapshot.len() as u32).to_le_bytes());
        msg.extend_from_slice(&snapshot);
        Some((slot, msg))
    }

    /// Marks a slot as leaving. The world is not touched until the next tick,
    /// so every client removes the player on the same frame.
    pub fn leave(&mut self, slot: usize) {
        if let Some(s) = self.slots.get_mut(slot) {
            if s.occupied {
                s.pending_leave = true;
                s.out = None;
                s.buttons = 0;
            }
        }
    }

    /// Records the latest input from a player.
    ///
    /// The most recent input wins rather than being queued per frame: a player
    /// whose packet is late should make everyone else wait for nothing.
    pub fn set_input(&mut self, slot: usize, buttons: u16) {
        if let Some(s) = self.slots.get_mut(slot) {
            if s.occupied {
                s.buttons = buttons;
            }
        }
    }

    /// Asks for a slot to take over a boss on the next tick.
    pub fn request_boss(&mut self, slot: usize) {
        if let Some(s) = self.slots.get_mut(slot) {
            if s.occupied {
                s.pending_boss = true;
            }
        }
    }

    /// Advances the world by one frame and returns the message describing it.
    pub fn tick(&mut self) -> Vec<u8> {
        let mut join_mask = 0u8;
        let mut boss_mask = 0u8;
        let mut leave_mask = 0u8;

        for i in 0..self.slots.len() {
            if self.slots[i].pending_leave {
                leave_mask |= 1 << i;
            } else if self.slots[i].pending_boss {
                boss_mask |= 1 << i;
            } else if self.slots[i].pending_join {
                join_mask |= 1 << i;
            }
        }

        // Apply the same world changes the clients are about to apply.
        for i in 0..self.slots.len() {
            if leave_mask & (1 << i) != 0 {
                self.world.leave(i);
                self.slots[i] = Slot::default();
            } else if boss_mask & (1 << i) != 0 {
                if !self.world.possess_boss(i) {
                    // No boss to take: come in as a hero instead.
                    self.world.join(i);
                    join_mask |= 1 << i;
                    boss_mask &= !(1 << i);
                }
                self.slots[i].pending_boss = false;
                self.slots[i].pending_join = false;
            } else if join_mask & (1 << i) != 0 {
                self.world.join(i);
                self.slots[i].pending_join = false;
            }
        }

        for i in 0..self.slots.len() {
            self.world.set_input(i, self.slots[i].buttons);
        }
        self.world.step();
        self.frame = self.frame.wrapping_add(1);

        let sum = self.world.checksum();
        self.checks.push_back((self.frame, sum));
        while self.checks.len() > CHECK_HISTORY {
            self.checks.pop_front();
        }

        let mut msg = Vec::with_capacity(8 + MAX_PLAYERS * 2);
        msg.push(s2c::FRAME);
        msg.extend_from_slice(&self.frame.to_le_bytes());
        msg.push(self.occupancy());
        msg.push(join_mask);
        msg.push(boss_mask);
        msg.push(leave_mask);
        for i in 0..MAX_PLAYERS {
            let buttons = self.slots.get(i).map(|s| s.buttons).unwrap_or(0);
            msg.extend_from_slice(&buttons.to_le_bytes());
        }
        msg
    }

    /// Compares a client's checksum against the server's own.
    ///
    /// Returns a message to send back when they disagree, which means that
    /// client is no longer playing the same game as everyone else.
    pub fn check(&mut self, frame: u32, theirs: u64) -> Option<Vec<u8>> {
        let ours = self.checks.iter().find(|(f, _)| *f == frame)?.1;
        if ours == theirs {
            return None;
        }
        self.desyncs += 1;
        let mut msg = vec![s2c::DESYNC];
        msg.extend_from_slice(&frame.to_le_bytes());
        msg.extend_from_slice(&ours.to_le_bytes());
        Some(msg)
    }

    /// Sends a message to every connected client, dropping any whose channel
    /// has gone away.
    pub fn broadcast(&mut self, msg: &[u8]) {
        for slot in self.slots.iter_mut() {
            if let Some(out) = &slot.out {
                if out.send(msg.to_vec()).is_err() {
                    slot.out = None;
                }
            }
        }
    }

    /// Sends a message to one client.
    pub fn send_to(&self, slot: usize, msg: &[u8]) {
        if let Some(Some(out)) = self.slots.get(slot).map(|s| &s.out) {
            let _ = out.send(msg.to_vec());
        }
    }

    /// True when nobody is connected.
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|s| !s.occupied)
    }
}

/// Builds a short text message for the client's status line.
pub fn info(text: &str) -> Vec<u8> {
    let mut msg = vec![s2c::INFO];
    msg.extend_from_slice(text.as_bytes());
    msg
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    fn session_with_players(n: usize) -> (Session, Vec<std::sync::mpsc::Receiver<Vec<u8>>>) {
        let mut s = Session::new(99);
        let mut rxs = Vec::new();
        for _ in 0..n {
            let (tx, rx) = channel();
            s.join(tx, Role::Hero).expect("a slot should be free");
            rxs.push(rx);
        }
        (s, rxs)
    }

    #[test]
    fn players_take_the_first_free_slot() {
        let (mut s, _rx) = session_with_players(2);
        assert_eq!(s.player_count(), 2);
        assert_eq!(s.occupancy(), 0b11);
        s.leave(0);
        s.tick();
        assert_eq!(s.occupancy(), 0b10);
        let (tx, _rx) = channel();
        let (slot, _) = s.join(tx, Role::Hero).unwrap();
        assert_eq!(slot, 0, "the freed slot should be reused");
    }

    #[test]
    fn a_full_session_turns_people_away() {
        let (mut s, _rx) = session_with_players(MAX_PLAYERS);
        let (tx, _rx) = channel();
        assert!(s.join(tx, Role::Hero).is_none());
    }

    #[test]
    fn the_first_player_gets_no_snapshot_but_a_later_one_does() {
        let mut s = Session::new(4);
        let (tx, _rx) = channel();
        let (_, welcome) = s.join(tx, Role::Hero).unwrap();
        let snapshot_len = |w: &[u8]| u32::from_le_bytes([w[15], w[16], w[17], w[18]]) as usize;
        assert_eq!(snapshot_len(&welcome), 0, "nothing has happened yet");

        for _ in 0..30 {
            s.tick();
        }
        let (tx, _rx) = channel();
        let (_, welcome) = s.join(tx, Role::Hero).unwrap();
        let len = snapshot_len(&welcome);
        assert!(len > 0, "a late joiner needs the state so far");
        assert_eq!(welcome.len(), WELCOME_HEADER + len);
    }

    #[test]
    fn a_frame_message_carries_every_slot_and_its_commands() {
        let (mut s, _rx) = session_with_players(2);
        s.set_input(1, 0x0f);
        let msg = s.tick();
        assert_eq!(msg[0], s2c::FRAME);
        assert_eq!(u32::from_le_bytes([msg[1], msg[2], msg[3], msg[4]]), 1);
        assert_eq!(msg[5], 0b11, "both slots occupied");
        assert_eq!(msg[6], 0b11, "both joined on this frame");
        let buttons_1 = u16::from_le_bytes([msg[11], msg[12]]);
        assert_eq!(buttons_1, 0x0f);
        assert_eq!(msg.len(), 9 + MAX_PLAYERS * 2);
    }

    #[test]
    fn a_client_replaying_the_frames_lands_in_the_same_place() {
        // This is the property the whole design rests on.
        let (mut server, _rx) = session_with_players(2);
        let mut client = zelduh_gen::new_world(Config::from_seed(server.seed), MAX_PLAYERS);

        for f in 0..400u32 {
            server.set_input(0, (f as u16 * 13) & 0x3f);
            server.set_input(1, (f as u16 * 7) & 0x0f);
            let msg = server.tick();

            // What a client does with the message it just received.
            let join_mask = msg[6];
            let boss_mask = msg[7];
            let leave_mask = msg[8];
            for i in 0..MAX_PLAYERS {
                if leave_mask & (1 << i) != 0 {
                    client.leave(i);
                } else if boss_mask & (1 << i) != 0 {
                    client.possess_boss(i);
                } else if join_mask & (1 << i) != 0 {
                    client.join(i);
                }
            }
            for i in 0..MAX_PLAYERS {
                let at = 9 + i * 2;
                client.set_input(i, u16::from_le_bytes([msg[at], msg[at + 1]]));
            }
            client.step();
        }
        assert_eq!(
            server.world.checksum(),
            client.checksum(),
            "a client following the frame messages must stay in step"
        );
    }

    #[test]
    fn a_late_joiner_catches_up_from_the_snapshot() {
        let (mut server, _rx) = session_with_players(1);
        for f in 0..200u32 {
            server.set_input(0, (f as u16 * 11) & 0x3f);
            server.tick();
        }

        let (tx, _rx) = channel();
        let (slot, welcome) = server.join(tx, Role::Hero).unwrap();
        let len = u32::from_le_bytes([welcome[15], welcome[16], welcome[17], welcome[18]]) as usize;
        let snapshot = &welcome[WELCOME_HEADER..WELCOME_HEADER + len];
        let mut client = World::load(snapshot).expect("the snapshot should load");
        assert_eq!(client.checksum(), server.world.checksum());

        for f in 0..120u32 {
            server.set_input(0, (f as u16 * 5) & 0x3f);
            server.set_input(slot, (f as u16 * 3) & 0x0f);
            let msg = server.tick();
            let join_mask = msg[6];
            for i in 0..MAX_PLAYERS {
                if join_mask & (1 << i) != 0 {
                    client.join(i);
                }
            }
            for i in 0..MAX_PLAYERS {
                let at = 9 + i * 2;
                client.set_input(i, u16::from_le_bytes([msg[at], msg[at + 1]]));
            }
            client.step();
        }
        assert_eq!(
            server.world.checksum(),
            client.checksum(),
            "a player who joined late must end up in the same world"
        );
    }

    #[test]
    fn a_player_can_join_as_the_boss() {
        let mut s = Session::new(7);
        let (tx, _rx) = channel();
        s.join(tx, Role::Hero).unwrap();
        let (tx, _rx) = channel();
        let (slot, _) = s.join(tx, Role::Boss).unwrap();
        let msg = s.tick();
        assert_eq!(
            msg[7] & (1 << slot),
            1 << slot,
            "the boss bit should be set"
        );
        assert_eq!(s.world.players[slot].role, zelduh_core::Role::Boss);
    }

    #[test]
    fn checksums_only_complain_when_they_disagree() {
        let (mut s, _rx) = session_with_players(1);
        for _ in 0..10 {
            s.tick();
        }
        let good = s.world.checksum();
        assert!(s.check(10, good).is_none());
        let reply = s
            .check(10, good ^ 1)
            .expect("a mismatch should be reported");
        assert_eq!(reply[0], s2c::DESYNC);
        assert_eq!(s.desyncs, 1);
        assert!(s.check(9999, good).is_none(), "unknown frames are ignored");
    }

    #[test]
    fn a_disconnected_client_is_dropped_from_broadcasts() {
        let (mut s, rxs) = session_with_players(2);
        drop(rxs);
        s.broadcast(b"hello");
        // The second broadcast must not panic now the receivers are gone.
        s.broadcast(b"still here");
    }
}
