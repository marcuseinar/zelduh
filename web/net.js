// Multiplayer, peer to peer.
//
// There is no game server. Peers find each other through a public
// matchmaking relay and then talk directly over WebRTC data channels, which
// is why the game works from static hosting: GitHub Pages serves the files
// and has nothing else to do with the session. Matchmaking is Trystero's
// job, vendored under `vendor/trystero`; everything below it is this file.
//
// The model is deterministic lockstep with delayed input. Nobody sends a
// position or a health bar: every peer runs the same simulation and the only
// thing on the wire is which buttons were held on which frame. A frame is
// simulated once every peer's buttons for it have arrived, and a peer sends
// its buttons a few frames ahead of the one it is simulating so that they
// are usually already there when needed. The engine has no floating point
// and no clock of its own, so the same buttons really do produce the same
// world, which a periodic checksum swap checks rather than assumes.
//
// One peer — whichever has the lowest id, so everyone picks the same one
// without being told — acts as the arbiter. It hands out player slots and
// stamps joins and departures with the frame they take effect on, so that
// everybody changes the shape of the game on the same frame. It is not a
// server: it holds no authority over the simulation, and if it disappears
// the next-lowest id takes over without anything else changing.

import { joinRoom as joinNostr, selfId } from './vendor/trystero/nostr.mjs';
import { joinRoom as joinTorrent } from './vendor/trystero/torrent.mjs';
import { joinRoom as joinRelay } from './vendor/trystero/ws-relay.mjs';

export { selfId };

/** How the room is found. Peer traffic never touches any of these. */
export const STRATEGIES = {
  nostr: { label: 'Nostr relays', join: joinNostr },
  torrent: { label: 'BitTorrent trackers', join: joinTorrent },
  relay: { label: 'Your own server', join: joinRelay },
};

const APP_ID = 'zelduh';
/** Slots in a world. */
export const PLAYERS = 4;
/** One frame of the simulation, in milliseconds. */
const FRAME_MS = 1000 / 60;
/**
 * How far ahead of the frame it is simulating a peer sends its buttons.
 * Four frames is 67ms of slack to cover the trip, at the cost of the same
 * delay between pressing a button and seeing it happen.
 */
const INPUT_DELAY = 4;
/**
 * How far ahead a join or a departure is stamped. It only has to be further
 * than [`INPUT_DELAY`], but a wide margin means a joiner has time to restore
 * a snapshot before the frame it comes alive on.
 */
const COMMAND_DELAY = 30;
/** How many already-sent frames ride along with each input message. */
const REDUNDANCY = 4;
/** Frames between checksum swaps. */
const CHECKSUM_EVERY = 120;
/** Never simulate more than this many frames in one animation frame. */
const MAX_FRAMES_PER_TICK = 8;
/** Frames of input kept behind the current one before being forgotten. */
const HISTORY = 240;
/**
 * How long a peer waits after opening a room before deciding it is alone.
 *
 * Without it, everyone who joins is briefly the only peer they know about
 * and claims the room, and two claims have to be unwound. Waiting a couple
 * of seconds for the relay to introduce everybody means the common case —
 * joining a room somebody is already in — never claims anything.
 */
const SETTLE_MS = 2000;
/** How often an unseated peer asks again for a place. */
const ASK_EVERY_MS = 1000;
/** Unanswered requests before the peer being asked is presumed gone. */
const UNANSWERED_LIMIT = 6;
/**
 * How long everyone waits on a silent peer before writing it off.
 *
 * WebRTC can take the better part of a minute to admit that a connection has
 * died, which is a long time to stand still in a game. Counted in animation
 * frames, so about four seconds.
 */
const PATIENCE = 240;

/** Words that make a room code easy to read out over the phone. */
const WORDS = [
  'acorn', 'amber', 'anvil', 'badger', 'bramble', 'cavern', 'cinder', 'copper',
  'dagger', 'ember', 'fable', 'falcon', 'garnet', 'gopher', 'harbour', 'hazel',
  'ivory', 'jasper', 'kettle', 'lantern', 'marble', 'meadow', 'nettle', 'onyx',
  'otter', 'pebble', 'quiver', 'raven', 'ripple', 'saffron', 'shroud', 'thistle',
  'tinder', 'umber', 'velvet', 'walnut', 'willow', 'yarrow', 'zephyr', 'cobble',
];

/** A room code somebody can read out loud. */
export function randomRoom() {
  const pick = () => WORDS[Math.floor(Math.random() * WORDS.length)];
  return `${pick()}-${pick()}-${Math.floor(Math.random() * 90 + 10)}`;
}

/** Everything the netcode needs from the engine. */
class Net {
  constructor(wasm, hooks) {
    this.wasm = wasm;
    this.hooks = hooks;
    this.room = null;
    this.reset();
  }

  reset() {
    this.frame = 0;
    this.slot = -1;
    /** Buttons per slot, keyed by frame. */
    this.inputs = Array.from({ length: PLAYERS }, () => new Map());
    /** Which slots the simulation currently has someone in. */
    this.active = new Array(PLAYERS).fill(false);
    /** Peer id per slot, as decided by the arbiter. */
    this.seats = new Array(PLAYERS).fill(null);
    /** Pending joins and departures, keyed by the frame they land on. */
    this.commands = new Map();
    /** Our own checksums, kept until a peer's arrives to compare. */
    this.sums = new Map();
    this.peers = [];
    this.lastSent = -1;
    this.accumulator = 0;
    this.stalled = 0;
    this.desynced = false;
    this.seated = false;
    this.held = 0;
    this.joinedAt = 0;
    this.lastAsk = 0;
    this.wasArbiter = false;
    this.selfSeated = false;
    this.askedAgain = false;
    this.asks = 0;
    /**
     * True once this peer is simulating the shared world rather than one of
     * its own. Holding a seat is not enough: a joiner has a place in the
     * room a moment before the snapshot that goes with it arrives, and
     * stepping in lockstep before then would be stepping the wrong world.
     */
    this.synced = false;
  }

  get online() {
    return !!this.room;
  }

  /** The peer everyone agrees to take slot assignments from. */
  get arbiter() {
    return [selfId, ...this.peers].sort()[0];
  }

  get isArbiter() {
    return this.arbiter === selfId;
  }

  // ----------------------------------------------------------- joining

  async join(code, { strategy = 'nostr', relayUrls = [], role = 'hero' } = {}) {
    this.leave();
    const chosen = STRATEGIES[strategy] ?? STRATEGIES.nostr;
    const config = { appId: APP_ID };
    if (relayUrls.length) config.relayConfig = { urls: relayUrls };
    if (strategy === 'relay' && !relayUrls.length) {
      throw new Error('your own server needs an address');
    }
    this.reset();
    this.role = role;
    this.frame = this.wasm.zelduh_frame();
    this.refreshActive();
    this.room = chosen.join(config, roomId(code));

    this.inputAction = this.room.makeAction('in');
    this.commandAction = this.room.makeAction('cmd');
    this.helloAction = this.room.makeAction('hi');
    this.welcomeAction = this.room.makeAction('yo');
    this.sumAction = this.room.makeAction('sum');

    // A message is proof its sender is here, and it can beat the event that
    // says so. Noting the peer first means no handler ever judges a message
    // against a roster that has not caught up with it.
    const from = (handler) => (payload, ctx) => {
      this.note(ctx.peerId);
      handler(payload, ctx);
    };
    this.inputAction.onMessage = from((data, ctx) => this.takeInputs(data, ctx.peerId));
    this.commandAction.onMessage = from((cmd, ctx) => this.takeCommand(cmd, ctx.peerId));
    this.helloAction.onMessage = from((hi, ctx) => this.takeHello(hi, ctx.peerId));
    this.welcomeAction.onMessage = from((snap, ctx) => this.takeWelcome(snap, ctx));
    this.sumAction.onMessage = from((sum, ctx) => this.takeChecksum(sum, ctx.peerId));

    this.room.onPeerJoin = (id) => {
      // Trust the event over any roster the room keeps: a connection can be
      // announced a moment before it is listed.
      if (!this.peers.includes(id)) this.peers.push(id);
      this.onRoster();
    };
    this.room.onPeerLeave = (id) => {
      this.peers = this.peers.filter((p) => p !== id);
      this.departed(id);
      this.onRoster();
    };

    this.joinedAt = now();
    this.onRoster();
    return roomId(code);
  }

  leave() {
    if (this.room) {
      try {
        this.room.leave();
      } catch {
        // Leaving a room that has already fallen over is not a problem.
      }
    }
    this.room = null;
    this.reset();
    this.hooks.onState?.();
  }

  /** Records a peer we have heard from, if the roster has not yet. */
  note(peerId) {
    if (!peerId || peerId === selfId || this.peers.includes(peerId)) return;
    this.peers.push(peerId);
    this.onRoster();
  }

  /** Called whenever the set of peers changes. */
  onRoster() {
    const arbiter = this.arbiter;
    if (this.selfSeated && arbiter !== selfId) {
      // Somebody older is here, so the place we gave ourselves while alone
      // was a guess. Only that guess is withdrawn: anything the real
      // arbiter has already told us stands.
      const mine = this.seats.indexOf(selfId);
      if (mine >= 0) this.seats[mine] = null;
      this.selfSeated = false;
      this.synced = false;
      this.seated = false;
      this.slot = -1;
      this.lastAsk = 0;
    }
    this.wasArbiter = arbiter === selfId;
    this.reconcile();
  }

  /**
   * Settles who is sitting where.
   *
   * Only the arbiter hands out places, and only once it has waited long
   * enough to be sure it really is the arbiter. Everyone else either finds
   * themselves in the map it published or asks to be put in it.
   */
  reconcile() {
    if (!this.room) return;
    const mine = this.seats.indexOf(selfId);
    if (mine >= 0) {
      this.slot = mine;
      this.seated = true;
      // Inheriting the room means the world here is now the shared one,
      // even if it arrived from somebody who has since gone: whoever else
      // is left will ask for it and be sent a snapshot of this copy.
      if (this.isArbiter) this.synced = true;
    } else if (this.isArbiter && now() - this.joinedAt > SETTLE_MS) {
      // Nobody older turned up, so the world already running here is the one
      // everybody who arrives will share.
      this.claimRoom();
    } else {
      this.seated = false;
      this.slot = -1;
    }
    // Keep asking until the world itself has arrived, not merely a seat.
    if (!this.synced) this.ask();
    this.hooks.onState?.();
  }

  /**
   * Takes the room over: the world already running here becomes the shared
   * one, and this peer takes the seat it is already playing in.
   *
   * A locally started world always puts its player in the first slot, and a
   * peer that was seated by somebody else never gets here, because it is
   * already in the seat map.
   */
  claimRoom() {
    if (this.seats.includes(selfId)) return;
    this.selfSeated = true;
    this.synced = true;
    this.seat(0, selfId);
  }

  /** Asks the arbiter for a place, but not more than once a second. */
  ask() {
    if (this.isArbiter || !this.peers.length) return;
    if (now() - this.lastAsk < ASK_EVERY_MS) return;
    this.lastAsk = now();
    this.asks += 1;
    if (this.asks > UNANSWERED_LIMIT) {
      // Asking into the void. Whoever this is, they are not there any more,
      // which usually makes the asker the new arbiter.
      const gone = this.arbiter;
      this.asks = 0;
      this.peers = this.peers.filter((p) => p !== gone);
      if (this.isArbiter) {
        this.departed(gone);
        this.onRoster();
      }
      return;
    }
    this.helloAction.send({ role: this.role }, { target: this.arbiter });
  }

  seat(slot, peer, why) {
    this.seats[slot] = peer;
    if (peer === selfId) {
      this.slot = slot;
      this.seated = true;
      this.lastSent = Math.max(this.lastSent, this.frame - 1);
    }
    this.hooks.onState?.(why);
  }

  freeSeat() {
    for (let i = 0; i < PLAYERS; i += 1) {
      if (!this.seats[i] && !this.active[i]) return i;
    }
    return -1;
  }

  /** Slots the arbiter has promised but the world has not filled yet. */
  get pendingSeats() {
    return this.seats.filter(Boolean).length;
  }

  // ------------------------------------------------- arbiter: seating

  takeHello(hi, peer) {
    if (!this.isArbiter || !this.room) return;
    // Being asked for a place is somebody else deciding this peer runs the
    // room, which settles the question early: a seat may not be handed out
    // before the arbiter has taken its own, or the map that goes out with
    // it would not say where to find the arbiter.
    this.claimRoom();
    // Asking again means the last answer never arrived. Take back the place
    // that was set aside and start the introduction over, rather than
    // leaving them holding a seat in a world they have never seen.
    const held = this.seats.indexOf(peer);
    if (held >= 0) {
      this.cancelPending(held);
      this.seats[held] = null;
    }
    const slot = this.freeSeat();
    if (slot < 0) {
      this.welcomeAction.send(pack({ full: true }, new Uint8Array(0)), { target: peer });
      return;
    }
    this.seat(slot, peer);
    const at = this.frame + COMMAND_DELAY;
    const kind = hi && hi.role === 'boss' ? 'boss' : 'join';
    this.issue({ f: at, kind, slot, seats: this.seats.slice() });

    // The world as it stands, so they can catch up to this moment and then
    // keep in step from the input alone.
    const len = this.wasm.zelduh_save();
    const snapshot = new Uint8Array(
      this.wasm.memory.buffer,
      this.wasm.zelduh_snapshot(),
      len,
    ).slice();
    const header = {
      slot,
      frame: this.frame,
      seedLo: this.wasm.zelduh_seed_lo(),
      seedHi: this.wasm.zelduh_seed_hi(),
      seats: this.seats.slice(),
      commands: this.upcoming(),
    };
    this.welcomeAction.send(pack(header, snapshot), { target: peer });
  }

  /** Forgets any join or boss command for a slot that has not landed yet. */
  cancelPending(slot) {
    for (const [f, list] of this.commands) {
      const kept = list.filter((c) => c.slot !== slot || c.kind === 'leave');
      if (kept.length) this.commands.set(f, kept);
      else this.commands.delete(f);
    }
  }

  /** Commands that have not landed yet, for a peer that is catching up. */
  upcoming() {
    const out = [];
    for (const [f, list] of this.commands) {
      if (f >= this.frame) out.push(...list);
    }
    return out;
  }

  /** Broadcasts a command and keeps a copy. */
  issue(cmd) {
    this.commandAction.send(cmd);
    this.takeCommand(cmd, selfId);
  }

  takeCommand(cmd, from) {
    if (!cmd || typeof cmd.f !== 'number') return;
    // Only the arbiter may reshape the game, and only once per frame stamp.
    if (from !== selfId && from !== this.arbiter) return;
    const list = this.commands.get(cmd.f) ?? [];
    if (list.some((c) => c.kind === cmd.kind && c.slot === cmd.slot)) return;
    list.push(cmd);
    // Two commands landing on one frame are applied in slot order, so that
    // every peer applies them in the same order.
    list.sort((a, b) => a.slot - b.slot || a.kind.localeCompare(b.kind));
    this.commands.set(cmd.f, list);

    if (cmd.seats) this.seats = cmd.seats.slice();
    if (cmd.seats && cmd.seats[cmd.slot] === selfId && cmd.kind !== 'leave') {
      this.slot = cmd.slot;
      this.seated = true;
    }
    // A departing peer's last buttons come with the command, so that every
    // peer fills the gap identically instead of guessing.
    if (cmd.kind === 'leave' && Array.isArray(cmd.pad)) {
      cmd.pad.forEach((buttons, i) => {
        const f = cmd.from + i;
        if (f >= this.frame) this.inputs[cmd.slot].set(f, buttons);
      });
    }
    this.hooks.onState?.();
  }

  /** A peer vanished: the arbiter decides when the world notices. */
  departed(peer) {
    const slot = this.seats.indexOf(peer);
    if (slot < 0) return;
    if (!this.isArbiter) return;
    const at = this.frame + COMMAND_DELAY;
    const pad = [];
    for (let f = this.frame; f < at; f += 1) pad.push(this.inputs[slot].get(f) ?? 0);
    this.seats[slot] = null;
    this.issue({ f: at, kind: 'leave', slot, from: this.frame, pad, seats: this.seats.slice() });
  }

  // ------------------------------------------------- joiner: catching up

  takeWelcome(message, ctx) {
    if (ctx.peerId !== this.arbiter) return;
    const welcome = unpack(message);
    if (!welcome) return;
    const [head, snapshot] = welcome;
    if (head.full) {
      this.hooks.onState?.('The room is full.');
      return;
    }
    if (typeof head.frame !== 'number' || typeof head.slot !== 'number') return;
    const seed = (BigInt(head.seedHi >>> 0) << 32n) | BigInt(head.seedLo >>> 0);
    this.wasm.zelduh_new_game(head.seedLo, head.seedHi, PLAYERS);
    const bytes = snapshot;
    if (!this.hooks.restore(bytes)) {
      this.hooks.onState?.('That world came from a different build of the game.');
      this.leave();
      return;
    }
    this.frame = head.frame;
    this.seats = (head.seats ?? []).slice();
    this.slot = head.slot;
    this.seated = true;
    this.synced = true;
    this.selfSeated = false;
    this.askedAgain = false;
    this.asks = 0;
    this.desynced = false;
    this.sums.clear();
    this.accumulator = 0;
    this.refreshActive();
    for (const cmd of head.commands ?? []) this.takeCommand(cmd, ctx.peerId);

    // Nothing is pressed between arriving and coming alive, and saying so up
    // front means nobody ever waits on a peer that is still reading a
    // snapshot.
    const wake = [...this.commands.keys()].sort((a, b) => a - b).at(-1) ?? this.frame;
    const quiet = wake + INPUT_DELAY;
    for (let f = this.frame; f <= quiet; f += 1) this.inputs[this.slot].set(f, 0);
    this.lastSent = quiet;
    this.sendRange(this.frame, quiet);
    this.hooks.onState?.(`Joined at frame ${head.frame}.`);
    this.hooks.onSeed?.(seed);
  }

  // ------------------------------------------------------------ inputs

  /** Packs a run of frames as [frame u32][count u8][buttons u16 ...]. */
  sendRange(first, last) {
    const count = Math.min(last - first + 1, 255);
    if (!Number.isInteger(count) || count <= 0 || this.slot < 0) return;
    const msg = new Uint8Array(5 + count * 2);
    const view = new DataView(msg.buffer);
    view.setUint32(0, first, true);
    view.setUint8(4, count);
    for (let i = 0; i < count; i += 1) {
      view.setUint16(5 + i * 2, this.inputs[this.slot].get(first + i) ?? 0, true);
    }
    this.inputAction.send(msg);
  }

  /** Records and broadcasts our buttons for the frames not yet spoken for. */
  sendInputs() {
    if (this.slot < 0) return;
    const target = this.frame + INPUT_DELAY;
    if (target <= this.lastSent) return;
    for (let f = this.lastSent + 1; f <= target; f += 1) {
      this.inputs[this.slot].set(f, this.held);
    }
    const first = Math.max(this.frame, this.lastSent + 1 - REDUNDANCY);
    this.lastSent = target;
    this.sendRange(first, target);
  }

  takeInputs(data, peer) {
    const slot = this.seats.indexOf(peer);
    if (slot < 0 || !(data instanceof Uint8Array) || data.length < 5) return;
    const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
    const first = view.getUint32(0, true);
    const count = view.getUint8(4);
    for (let i = 0; i < count && 5 + i * 2 + 1 < data.length; i += 1) {
      const f = first + i;
      // A frame already simulated is settled; nothing may rewrite it.
      if (f < this.frame) continue;
      this.inputs[slot].set(f, view.getUint16(5 + i * 2, true));
    }
  }

  // -------------------------------------------------------------- clock

  refreshActive() {
    for (let i = 0; i < PLAYERS; i += 1) {
      this.active[i] = !!this.wasm.zelduh_player_active(i);
    }
  }

  /** Slots whose buttons frame `f` cannot be simulated without. */
  needed(f) {
    const need = new Set();
    for (let i = 0; i < PLAYERS; i += 1) if (this.active[i]) need.add(i);
    for (const cmd of this.commands.get(f) ?? []) {
      if (cmd.kind === 'leave') need.delete(cmd.slot);
      else need.add(cmd.slot);
    }
    return need;
  }

  ready(f) {
    for (const slot of this.needed(f)) {
      if (!this.inputs[slot].has(f)) return false;
    }
    return true;
  }

  stepOnce() {
    const f = this.frame;
    for (const cmd of this.commands.get(f) ?? []) {
      if (cmd.kind === 'leave') this.wasm.zelduh_leave(cmd.slot);
      else if (cmd.kind === 'boss') this.wasm.zelduh_possess_boss(cmd.slot);
      else this.wasm.zelduh_join(cmd.slot);
    }
    this.commands.delete(f);
    this.refreshActive();

    for (let i = 0; i < PLAYERS; i += 1) {
      this.wasm.zelduh_set_input(i, this.active[i] ? this.inputs[i].get(f) ?? 0 : 0);
    }
    this.wasm.zelduh_step();
    this.hooks.onStepped?.();

    this.frame += 1;
    for (const map of this.inputs) map.delete(f - HISTORY);
    if (this.frame % CHECKSUM_EVERY === 0) this.swapChecksum();
  }

  /**
   * Advances the world by as many frames as the clock and the other peers
   * allow. Returns how many were simulated.
   */
  pump(dt, buttons) {
    this.held = buttons;
    this.accumulator += dt;
    let stepped = 0;

    // Still being introduced. Carry on with the world that is here; it will
    // be replaced wholesale by a snapshot if somebody else owns the room.
    if (!this.synced || this.slot < 0) {
      this.reconcile();
      while (this.accumulator >= FRAME_MS && stepped < MAX_FRAMES_PER_TICK) {
        this.wasm.zelduh_set_input(0, this.held);
        this.wasm.zelduh_step();
        this.hooks.onStepped?.();
        this.accumulator -= FRAME_MS;
        stepped += 1;
      }
      this.frame = this.wasm.zelduh_frame();
      if (this.accumulator >= FRAME_MS) this.accumulator = 0;
      return stepped;
    }

    while (this.accumulator >= FRAME_MS && stepped < MAX_FRAMES_PER_TICK) {
      this.sendInputs();
      if (!this.ready(this.frame)) break;
      this.stepOnce();
      this.accumulator -= FRAME_MS;
      stepped += 1;
    }
    if (stepped === 0 && this.accumulator >= FRAME_MS) {
      // Waiting on somebody. Do not bank the debt, or catching up later
      // would be a lurch rather than a resumption.
      this.stalled += 1;
      this.accumulator = Math.min(this.accumulator, FRAME_MS * 2);
      if (this.stalled > PATIENCE) this.giveUpOn(this.blocking());
    } else {
      this.stalled = 0;
    }
    return stepped;
  }

  /** The first slot whose buttons the world is waiting for. */
  blocking() {
    for (const slot of this.needed(this.frame)) {
      if (!this.inputs[slot].has(this.frame)) return slot;
    }
    return -1;
  }

  /**
   * Stops waiting for a peer that has gone quiet.
   *
   * If that peer was the arbiter, dropping it from the roster promotes
   * whoever is left, and they carry on handing out places as though nothing
   * had happened.
   */
  giveUpOn(slot) {
    if (slot < 0) return;
    const peer = this.seats[slot];
    this.stalled = 0;
    if (!peer || peer === selfId) return;
    if (!this.isArbiter && !this.askedAgain) {
      // The fault may be at this end. Ask for the world again before
      // writing anybody off.
      this.askedAgain = true;
      this.synced = false;
      this.lastAsk = 0;
      this.ask();
      return;
    }
    this.peers = this.peers.filter((p) => p !== peer);
    this.askedAgain = false;
    if (!this.isArbiter) return;
    this.departed(peer);
    this.onRoster();
  }

  // ----------------------------------------------------------- checking

  swapChecksum() {
    const f = this.frame;
    const lo = this.wasm.zelduh_checksum_lo();
    const hi = this.wasm.zelduh_checksum_hi();
    this.sums.set(f, [lo, hi]);
    for (const key of this.sums.keys()) {
      if (key < f - CHECKSUM_EVERY * 4) this.sums.delete(key);
    }
    this.sumAction.send({ f, lo, hi });
  }

  takeChecksum({ f, lo, hi } = {}, peer) {
    const mine = this.sums.get(f);
    if (!mine || this.desynced) return;
    if (mine[0] === lo && mine[1] === hi) return;
    this.desynced = true;
    this.hooks.onState?.(
      `Out of step with ${peer.slice(0, 6)} at frame ${f}. Rejoin to catch up.`,
    );
  }

  /** Asks the arbiter to hand this player a boss to drive. */
  requestBoss() {
    if (!this.online) return;
    this.role = 'boss';
    if (this.isArbiter) {
      if (this.slot < 0) return;
      this.issue({
        f: this.frame + COMMAND_DELAY,
        kind: 'boss',
        slot: this.slot,
        seats: this.seats.slice(),
      });
    } else {
      this.helloAction.send({ role: 'boss' }, { target: this.arbiter });
    }
  }

  /** A one-line description of the session, for the page. */
  describe() {
    if (!this.online) return 'Playing on your own.';
    if (this.desynced) return 'Out of step. Rejoin to catch up.';
    if (!this.synced || this.slot < 0) return 'Looking for the room\u2026';
    const seated = this.seats.filter(Boolean).length;
    const who = this.slot >= 0 ? `Player ${this.slot + 1}` : 'Watching';
    const role = this.isArbiter ? ', holding the room open' : '';
    const waiting = this.stalled > 30 ? ' — waiting for someone' : '';
    return `${who} of ${Math.max(seated, 1)}${role}${waiting}.`;
  }
}

/**
 * Packs a description and a blob into one message.
 *
 * Everything the netcode sends travels in the message body rather than in
 * any side channel: one message either arrives whole or does not arrive, and
 * there is nothing to go missing between the two halves.
 */
function pack(header, blob) {
  const text = new TextEncoder().encode(JSON.stringify(header));
  const out = new Uint8Array(4 + text.length + blob.length);
  new DataView(out.buffer).setUint32(0, text.length, true);
  out.set(text, 4);
  out.set(blob, 4 + text.length);
  return out;
}

/** Undoes [`pack`], or returns null for anything that is not one. */
function unpack(message) {
  const bytes = message instanceof Uint8Array ? message : new Uint8Array(message ?? 0);
  if (bytes.length < 4) return null;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const len = view.getUint32(0, true);
  if (len > bytes.length - 4) return null;
  try {
    const header = JSON.parse(new TextDecoder().decode(bytes.subarray(4, 4 + len)));
    return [header, bytes.subarray(4 + len)];
  } catch {
    return null;
  }
}

/** A clock that cannot be dragged backwards by the system clock changing. */
function now() {
  return performance.now();
}

/** Trystero namespaces by room id; keep ours tidy and case-insensitive. */
function roomId(code) {
  return String(code).trim().toLowerCase().replace(/\s+/g, '-') || 'lobby';
}

export function createNet(wasm, hooks) {
  return new Net(wasm, hooks);
}
