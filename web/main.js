// The browser side of Zelduh.
//
// Rust owns the world and the pixels; this file owns input, files, sound and
// the canvas. Everything crosses the boundary as integers or as bytes in the
// module's memory, so there is no bindings generator in the build.

import { createNet, randomRoom, selfId, STRATEGIES } from './net.js';
import { createSound } from './sound.js';

const BUTTON = {
  up: 1 << 0,
  down: 1 << 1,
  left: 1 << 2,
  right: 1 << 3,
  a: 1 << 4,
  b: 1 << 5,
  start: 1 << 6,
  select: 1 << 7,
};

BUTTON.DPAD_ALL = BUTTON.up | BUTTON.down | BUTTON.left | BUTTON.right;

const KEYS = {
  ArrowUp: 'up', KeyW: 'up',
  ArrowDown: 'down', KeyS: 'down',
  ArrowLeft: 'left', KeyA: 'left',
  ArrowRight: 'right', KeyD: 'right',
  KeyZ: 'a', KeyJ: 'a', Space: 'a',
  KeyX: 'b', KeyK: 'b',
  Enter: 'start',
  ShiftLeft: 'select', ShiftRight: 'select',
};

// One frame of the simulation, in milliseconds. The Game Boy ran at 59.7Hz;
// 60 is close enough and lines up with most displays.
const FRAME_MS = 1000 / 60;
// Never simulate more than this many frames in one animation frame, so a
// backgrounded tab does not come back and freeze on a huge catch-up.
const MAX_CATCH_UP = 5;

const dom = {
  canvas: document.getElementById('screen'),
  overlay: document.getElementById('overlay'),
  overlayText: document.getElementById('overlay-text'),
  seed: document.getElementById('seed'),
  newWorld: document.getElementById('new-world'),
  randomSeed: document.getElementById('random-seed'),
  scale: document.getElementById('scale'),
  scaleLabel: document.getElementById('scale-label'),
  fill: document.getElementById('fill'),
  sound: document.getElementById('sound'),
  music: document.getElementById('music'),
  drop: document.getElementById('drop'),
  file: document.getElementById('file'),
  assetStatus: document.getElementById('asset-status'),
  resetAssets: document.getElementById('reset-assets'),
  stats: document.getElementById('stats'),
  connect: document.getElementById('connect'),
  netStatus: document.getElementById('net-status'),
  server: document.getElementById('server'),
  room: document.getElementById('room'),
  randomRoom: document.getElementById('random-room'),
  strategy: document.getElementById('strategy'),
  relayRow: document.getElementById('relay-row'),
  becomeBoss: document.getElementById('become-boss'),
  share: document.getElementById('share'),
  together: document.getElementById('together'),
  stage: document.getElementById('stage'),
  play: document.getElementById('play'),
  panel: document.getElementById('panel'),
  touchUi: document.getElementById('touch-ui'),
  stickZone: document.getElementById('stick-zone'),
  menu: document.getElementById('menu'),
  closePanel: document.getElementById('close-panel'),
  scrim: document.getElementById('sheet-scrim'),
  fullscreen: document.getElementById('fullscreen'),
};

/** True on a device driven by a thumb rather than a mouse. */
const TOUCH = matchMedia('(pointer: coarse)').matches;

const ctx = dom.canvas.getContext('2d', { alpha: false });
ctx.imageSmoothingEnabled = false;

let wasm = null;
let held = 0;
let paused = false;
let imageData = null;
const audio = createSound({
  wantsEffects: () => dom.sound.checked,
  wantsMusic: () => dom.music.checked,
});

// ---------------------------------------------------------------- wasm glue

/** Reads a stretch of the module's memory as bytes. */
function bytes(ptr, len) {
  if (!ptr || !len) return new Uint8Array(0);
  // The view has to be rebuilt every time: any allocation in Rust can grow
  // the module's memory, which detaches every existing view of it.
  return new Uint8Array(wasm.memory.buffer, ptr, len);
}

/** Reads a UTF-8 string out of the module. */
function text(ptrFn, lenFn) {
  const len = lenFn();
  if (!len) return '';
  return new TextDecoder().decode(bytes(ptrFn(), len).slice());
}

/** Copies bytes into the module and runs `f` with the pointer. */
function withBytes(data, f) {
  const ptr = wasm.zelduh_alloc(data.length);
  try {
    new Uint8Array(wasm.memory.buffer, ptr, data.length).set(data);
    return f(ptr, data.length);
  } finally {
    wasm.zelduh_free(ptr, data.length);
  }
}

async function loadWasm() {
  const response = await fetch('zelduh.wasm');
  if (!response.ok) throw new Error(`could not fetch zelduh.wasm (${response.status})`);
  let result;
  // instantiateStreaming needs the right content type; fall back when a
  // simple static server does not send it.
  try {
    result = await WebAssembly.instantiateStreaming(response.clone(), {});
  } catch {
    result = await WebAssembly.instantiate(await response.arrayBuffer(), {});
  }
  const e = result.instance.exports;
  return {
    memory: e.memory,
    zelduh_alloc: e.zelduh_alloc,
    zelduh_set_screen: e.zelduh_set_screen,
    zelduh_player_active: e.zelduh_player_active,
    zelduh_seed_lo: e.zelduh_seed_lo,
    zelduh_seed_hi: e.zelduh_seed_hi,
    zelduh_free: e.zelduh_free,
    zelduh_new_game: e.zelduh_new_game,
    zelduh_join: e.zelduh_join,
    zelduh_set_input: e.zelduh_set_input,
    zelduh_step: e.zelduh_step,
    zelduh_render: e.zelduh_render,
    zelduh_framebuffer: e.zelduh_framebuffer,
    zelduh_framebuffer_len: e.zelduh_framebuffer_len,
    zelduh_screen_width: e.zelduh_screen_width,
    zelduh_screen_height: e.zelduh_screen_height,
    zelduh_events: e.zelduh_events,
    zelduh_events_len: e.zelduh_events_len,
    zelduh_frame: e.zelduh_frame,
    zelduh_entity_count: e.zelduh_entity_count,
    zelduh_player_level: e.zelduh_player_level,
    zelduh_player_health: e.zelduh_player_health,
    zelduh_player_rupees: e.zelduh_player_rupees,
    zelduh_player_kills: e.zelduh_player_kills,
    zelduh_load_asset: e.zelduh_load_asset,
    zelduh_reset_assets: e.zelduh_reset_assets,
    zelduh_report: e.zelduh_report,
    zelduh_report_len: e.zelduh_report_len,
    zelduh_tile_count: e.zelduh_tile_count,
    zelduh_leave: e.zelduh_leave,
    zelduh_possess_boss: e.zelduh_possess_boss,
    zelduh_restore: e.zelduh_restore,
    zelduh_save: e.zelduh_save,
    zelduh_snapshot: e.zelduh_snapshot,
    zelduh_checksum_lo: e.zelduh_checksum_lo,
    zelduh_checksum_hi: e.zelduh_checksum_hi,
    zelduh_player_role: e.zelduh_player_role,
    zelduh_player_active: e.zelduh_player_active,
  };
}

// ------------------------------------------------------------------- audio

/** A small square-wave synth, in the spirit of the hardware being imitated. */
dom.music.addEventListener('change', () => audio.refresh());
dom.sound.addEventListener('change', () => audio.unlock());

/** Which tune belongs with what is happening. */
function trackForNow() {
  const me = online() && net.slot >= 0 ? net.slot : 0;
  if (!wasm.zelduh_player_active(me)) return 'overworld';
  if (wasm.zelduh_player_role(me) === 1) return 'boss';
  return wasm.zelduh_player_level(me) === 0 ? 'overworld' : 'dungeon';
}

// ------------------------------------------------------------------- input

function setButton(name, down) {
  const bit = BUTTON[name];
  if (!bit) return;
  if (down) held |= bit;
  else held &= ~bit;
}

window.addEventListener('keydown', (e) => {
  if (e.repeat) return;
  const name = KEYS[e.code];
  if (name) {
    setButton(name, true);
    audio.unlock();
    e.preventDefault();
  } else if (e.code === 'KeyP') {
    setPaused(!paused);
    e.preventDefault();
  }
});

window.addEventListener('keyup', (e) => {
  const name = KEYS[e.code];
  if (name) {
    setButton(name, false);
    e.preventDefault();
  }
});

// Holding a key and then clicking away leaves the button stuck down.
window.addEventListener('blur', () => { held = 0; });

// ------------------------------------------------------------ touch controls

// How hard the stick has to be pushed before it counts, as a fraction of its
// radius. Below this the hero stands still.
const STICK_DEADZONE = 0.35;
// A direction is included when the stick is within this many degrees of it,
// which leaves a generous wedge for each diagonal.
const STICK_SPREAD = 67.5;

/** Turns a stick vector into a set of direction bits. */
function stickDirections(vx, vy, force) {
  if (force < STICK_DEADZONE) return 0;
  // nipplejs hands back a vector with y pointing up, which is already the
  // convention for an angle in degrees.
  const angle = (Math.atan2(vy, vx) * 180) / Math.PI;
  const near = (target) => {
    const diff = Math.abs(((angle - target + 540) % 360) - 180);
    return diff <= STICK_SPREAD;
  };
  let mask = 0;
  if (near(90)) mask |= BUTTON.up;
  if (near(-90)) mask |= BUTTON.down;
  if (near(180)) mask |= BUTTON.left;
  if (near(0)) mask |= BUTTON.right;
  return mask;
}

/**
 * The movement stick, drawn wherever the thumb lands.
 *
 * nipplejs in `dynamic` mode is exactly the pattern every touch game uses:
 * no fixed pad to hunt for, the stick simply appears under the thumb and
 * recentres itself on every touch.
 */
function setupStick() {
  if (!window.nipplejs) return;
  const stick = nipplejs.create({
    zone: dom.stickZone,
    mode: 'dynamic',
    color: 'rgba(127, 208, 74, 0.85)',
    size: 120,
    fadeTime: 80,
    restOpacity: 0.65,
    // One thumb, one stick: a second finger in this half must not make
    // another one and fight the first for the d-pad bits.
    maxNumberOfNipples: 1,
    // The zone lives inside a fixed, full-screen layout, so nipplejs has to
    // place the stick from viewport coordinates rather than page ones.
    dynamicPage: true,
  });
  stick.on('start', () => audio.unlock());
  stick.on('move', (_event, data) => {
    if (!data || !data.vector) return;
    // Replace the whole d-pad at once so opposite directions cannot stick.
    held = (held & ~BUTTON.DPAD_ALL) | stickDirections(data.vector.x, data.vector.y, data.force);
  });
  stick.on('end', () => { held &= ~BUTTON.DPAD_ALL; });
  return stick;
}

function setupTouchControls() {
  dom.touchUi.hidden = false;
  document.body.classList.add('touch');
  setupStick();

  for (const el of document.querySelectorAll('[data-button]')) {
    const name = el.dataset.button;
    el.addEventListener('pointerdown', (e) => {
      e.preventDefault();
      audio.unlock();
      el.setPointerCapture(e.pointerId);
      el.classList.add('down');
      setButton(name, true);
    });
    const release = (e) => {
      e.preventDefault();
      el.classList.remove('down');
      setButton(name, false);
    };
    el.addEventListener('pointerup', release);
    el.addEventListener('pointercancel', release);
  }

  // A second tap in quick succession would otherwise zoom the page.
  for (const el of [dom.stickZone, dom.stage, ...document.querySelectorAll('[data-button]')]) {
    el.addEventListener('dblclick', (e) => e.preventDefault());
    el.addEventListener('contextmenu', (e) => e.preventDefault());
  }

  // Belt and braces for the page lock: iOS still rubber-bands a fixed body
  // when a gesture starts on an element that is not itself scrollable.
  document.addEventListener('touchmove', (e) => {
    if (e.cancelable && !e.target.closest('#panel')) e.preventDefault();
  }, { passive: false });
}

// ------------------------------------------------------------ settings sheet

function showPanel(open) {
  dom.panel.classList.toggle('open', open);
  dom.scrim.hidden = !open;
  requestAnimationFrame(applyScreen);
}

dom.menu.addEventListener('click', () => {
  audio.unlock();
  showPanel(!dom.panel.classList.contains('open'));
});
dom.closePanel.addEventListener('click', () => showPanel(false));
dom.scrim.addEventListener('click', () => showPanel(false));

dom.fullscreen.addEventListener('click', async () => {
  audio.unlock();
  try {
    if (document.fullscreenElement) {
      await document.exitFullscreen();
    } else {
      await document.documentElement.requestFullscreen({ navigationUI: 'hide' });
      // Landscape is the better shape for this screen, where it is allowed.
      if (screen.orientation?.lock) {
        try {
          await screen.orientation.lock('landscape');
        } catch {
          // Most browsers refuse unless installed; the page works either way.
        }
      }
    }
  } catch {
    // Fullscreen is not available everywhere, and refusing it is not an error.
  }
});

/** Folds any connected gamepad into the same button mask. */
function gamepadButtons() {
  if (!navigator.getGamepads) return 0;
  let mask = 0;
  for (const pad of navigator.getGamepads()) {
    if (!pad) continue;
    const [x = 0, y = 0] = pad.axes;
    if (y < -0.4 || pad.buttons[12]?.pressed) mask |= BUTTON.up;
    if (y > 0.4 || pad.buttons[13]?.pressed) mask |= BUTTON.down;
    if (x < -0.4 || pad.buttons[14]?.pressed) mask |= BUTTON.left;
    if (x > 0.4 || pad.buttons[15]?.pressed) mask |= BUTTON.right;
    if (pad.buttons[0]?.pressed) mask |= BUTTON.a;
    if (pad.buttons[1]?.pressed || pad.buttons[2]?.pressed) mask |= BUTTON.b;
    if (pad.buttons[9]?.pressed) mask |= BUTTON.start;
    if (pad.buttons[8]?.pressed) mask |= BUTTON.select;
  }
  return mask;
}

// ------------------------------------------------------------------- files

async function handleFile(file) {
  const name = file.name.toLowerCase();
  const isImage = file.type.startsWith('image/') && !name.endsWith('.bmp');
  try {
    if (isImage) {
      // The browser already has decoders for PNG and the rest, so the engine
      // never needs one: hand it the pixels.
      const bitmap = await createImageBitmap(file);
      const off = document.createElement('canvas');
      off.width = bitmap.width;
      off.height = bitmap.height;
      const c = off.getContext('2d');
      c.drawImage(bitmap, 0, 0);
      const { data, width, height } = c.getImageData(0, 0, off.width, off.height);
      withBytes(new Uint8Array(data.buffer), (ptr, len) =>
        wasm.zelduh_load_asset(ptr, len, 4, width, height));
    } else {
      const data = new Uint8Array(await file.arrayBuffer());
      withBytes(data, (ptr, len) => wasm.zelduh_load_asset(ptr, len, 0, 0, 0));
    }
    const detail = text(wasm.zelduh_report, wasm.zelduh_report_len);
    dom.assetStatus.textContent = `${file.name}: ${detail}`;
  } catch (err) {
    dom.assetStatus.textContent = `${file.name}: ${err.message}`;
  }
}

for (const type of ['dragenter', 'dragover']) {
  dom.drop.addEventListener(type, (e) => {
    e.preventDefault();
    dom.drop.classList.add('over');
  });
}
for (const type of ['dragleave', 'drop']) {
  dom.drop.addEventListener(type, (e) => {
    e.preventDefault();
    dom.drop.classList.remove('over');
  });
}
dom.drop.addEventListener('drop', (e) => {
  const file = e.dataTransfer?.files?.[0];
  if (file) handleFile(file);
});
// Dropping anywhere on the page is friendlier than aiming at the box.
window.addEventListener('dragover', (e) => e.preventDefault());
window.addEventListener('drop', (e) => {
  e.preventDefault();
  const file = e.dataTransfer?.files?.[0];
  if (file) handleFile(file);
});
dom.file.addEventListener('change', () => {
  if (dom.file.files[0]) handleFile(dom.file.files[0]);
});
dom.resetAssets.addEventListener('click', () => {
  wasm.zelduh_reset_assets();
  dom.assetStatus.textContent = text(wasm.zelduh_report, wasm.zelduh_report_len);
});

// ------------------------------------------------------------------ session

function startWorld(seed) {
  const s = BigInt.asUintN(64, BigInt(seed));
  const lo = Number(s & 0xffffffffn);
  const hi = Number((s >> 32n) & 0xffffffffn);
  wasm.zelduh_new_game(lo, hi, 4);
  wasm.zelduh_join(0);
  setPaused(false);
}

function parseSeed(value) {
  const trimmed = String(value).trim();
  if (/^-?\d+$/.test(trimmed)) return BigInt(trimmed);
  // Any other text becomes a seed of its own, so worlds can be named.
  let h = 0n;
  for (const ch of trimmed) {
    h = BigInt.asUintN(64, h * 1099511628211n ^ BigInt(ch.codePointAt(0)));
  }
  return h;
}

dom.newWorld.addEventListener('click', () => {
  audio.unlock();
  // A new local world means leaving the shared one.
  if (online()) {
    net.leave();
    refreshNetUi('Playing on your own.');
  }
  startWorld(parseSeed(dom.seed.value));
});
dom.randomSeed.addEventListener('click', () => {
  const n = Math.floor(Math.random() * 1e9);
  dom.seed.value = String(n);
  audio.unlock();
  startWorld(BigInt(n));
});

/** Sizes the canvas: whole-number zoom, or filling the space available. */
function applyScale() {
  const value = Number(dom.scale.value);
  if (value === 0) {
    dom.scaleLabel.textContent = 'Fit';
    fitCanvas();
  } else {
    dom.scaleLabel.textContent = `${value}x`;
    dom.canvas.style.width = `${dom.canvas.width * value}px`;
    dom.canvas.style.height = `${dom.canvas.height * value}px`;
  }
  try {
    localStorage.setItem('zelduh.scale', String(value));
  } catch {
    // Private windows can refuse storage; the zoom just will not be remembered.
  }
}

/** How much room there is for the screen, in CSS pixels. */
function availableBox() {
  // Measure with the canvas out of the way, so its own size does not decide
  // how much room there is for it.
  const was = [dom.canvas.style.width, dom.canvas.style.height];
  dom.canvas.style.width = '0px';
  dom.canvas.style.height = '0px';

  // Measure the container rather than the screen's own frame: with the canvas
  // collapsed the frame has shrunk to nothing and would report no room at all.
  const frame = getComputedStyle(dom.stage);
  const sides = (a, b) =>
    parseFloat(frame[`padding${a}`]) + parseFloat(frame[`padding${b}`]) +
    parseFloat(frame[`border${a}Width`]) + parseFloat(frame[`border${b}Width`]);

  const width = dom.play.clientWidth - sides('Left', 'Right');
  let height;
  if (TOUCH) {
    // The controls float over the picture, so the screen gets the whole page.
    height = dom.play.clientHeight - sides('Top', 'Bottom');
  } else {
    // The container's top, not the screen frame's: with the canvas collapsed
    // the frame is centred in its row and reports a position it will not keep.
    const top = dom.play.getBoundingClientRect().top;
    height = window.innerHeight - top - sides('Top', 'Bottom') - 16;
  }
  [dom.canvas.style.width, dom.canvas.style.height] = was;
  return { width: Math.max(width, 1), height: Math.max(height, 1) };
}

/** Makes the screen as large as it can be without pushing anything off. */
function fitCanvas() {
  const box = availableBox();
  const scale = Math.max(
    1,
    Math.min(box.width / dom.canvas.width, box.height / dom.canvas.height),
  );
  dom.canvas.style.width = `${Math.floor(dom.canvas.width * scale)}px`;
  dom.canvas.style.height = `${Math.floor(dom.canvas.height * scale)}px`;
}

// The Game Boy's own screen, which is the smallest the engine will draw.
const BASE_W = 160;
const BASE_H = 144;
// How much world the software renderer will fill sixty times a second. The
// Game Boy's screen is 23,040 pixels; a few times that is still comfortable,
// and past it a very long, thin display gets black bars back rather than a
// slideshow.
const MAX_VIEW_PIXELS = 130000;

/**
 * Picks how much of the world to draw.
 *
 * The world is continuous now, so a display that is not shaped like a Game
 * Boy can be shown more of it rather than bars down the sides. The scale is
 * the largest that still covers the whole box, which leaves one dimension at
 * the Game Boy's own size and stretches the other.
 */
function chooseScreen() {
  if (!dom.fill.checked) return [BASE_W, BASE_H];
  const box = availableBox();
  let scale = Math.min(box.width / BASE_W, box.height / BASE_H);
  if (!(scale > 0) || !Number.isFinite(scale)) return [BASE_W, BASE_H];
  let w = Math.round(box.width / scale);
  let h = Math.round(box.height / scale);
  if (w * h > MAX_VIEW_PIXELS) {
    const shrink = Math.sqrt((w * h) / MAX_VIEW_PIXELS);
    w = Math.max(BASE_W, Math.round(w / shrink));
    h = Math.max(BASE_H, Math.round(h / shrink));
  }
  return [w, h];
}

/** Resizes the screen to suit the display, then fits it to the page. */
function applyScreen() {
  // A resize can arrive before the module has finished loading.
  if (!wasm) return;
  const [w, h] = chooseScreen();
  if (wasm.zelduh_set_screen(w, h)) {
    dom.canvas.width = wasm.zelduh_screen_width();
    dom.canvas.height = wasm.zelduh_screen_height();
    ctx.imageSmoothingEnabled = false;
    // The old buffer is the wrong shape now.
    imageData = null;
  }
  applyScale();
}

dom.fill.addEventListener('change', () => {
  try {
    localStorage.setItem('zelduh.fill', dom.fill.checked ? '1' : '0');
  } catch {
    // Not remembering the preference is survivable.
  }
  applyScreen();
});

dom.scale.addEventListener('input', applyScale);
window.addEventListener('resize', () => requestAnimationFrame(applyScreen));
visualViewport?.addEventListener('resize', () => requestAnimationFrame(applyScreen));
window.addEventListener('orientationchange', () => {
  // The new viewport size is not known until after the rotation settles.
  setTimeout(applyScreen, 250);
});
document.addEventListener('fullscreenchange', () => requestAnimationFrame(applyScreen));
for (const el of document.querySelectorAll('#panel details')) {
  el.addEventListener('toggle', () => requestAnimationFrame(applyScreen));
}

function setPaused(value) {
  paused = value;
  dom.overlay.hidden = !value;
  if (value) dom.overlayText.textContent = 'Paused — press P to carry on';
}

document.addEventListener('visibilitychange', () => {
  if (document.hidden) setPaused(true);
});

dom.canvas.addEventListener('pointerdown', () => {
  audio.unlock();
  if (paused) setPaused(false);
});

// ----------------------------------------------------------------- network

let net = null;

/** True while a shared game is running. */
function online() {
  return !!net && net.online;
}

function setNetStatus(text, state = '') {
  dom.netStatus.textContent = text;
  dom.netStatus.className = state;
}

function refreshNetUi(message) {
  if (!net) return;
  const live = net.online;
  const boss = live && net.slot >= 0 && wasm.zelduh_player_role(net.slot) === 1;
  dom.connect.textContent = live ? 'Leave' : 'Join';
  dom.becomeBoss.disabled = !live || !net.synced || boss;
  dom.becomeBoss.textContent = boss ? 'You are the boss' : 'Play as the boss';
  dom.share.hidden = !live;
  if (live) {
    const url = new URL(location.href);
    url.hash = `room=${encodeURIComponent(dom.room.value.trim())}`;
    dom.share.textContent = `Share this address and they arrive in your world: ${url}`;
  }
  setNetStatus(
    message ?? net.describe(),
    net.desynced ? 'bad' : live ? 'live' : '',
  );
}

function setupNetUi() {
  for (const [key, { label }] of Object.entries(STRATEGIES)) {
    const option = document.createElement('option');
    option.value = key;
    option.textContent = label;
    dom.strategy.append(option);
  }
  dom.strategy.value = 'nostr';
  dom.strategy.addEventListener('change', () => {
    dom.relayRow.hidden = dom.strategy.value !== 'relay';
    remember('zelduh.strategy', dom.strategy.value);
  });

  dom.randomRoom.addEventListener('click', () => {
    dom.room.value = randomRoom();
  });

  dom.connect.addEventListener('click', async () => {
    audio.unlock();
    if (net.online) {
      net.leave();
      refreshNetUi('Playing on your own.');
      return;
    }
    if (!dom.room.value.trim()) dom.room.value = randomRoom();
    const code = dom.room.value.trim();
    remember('zelduh.room', code);
    remember('zelduh.server', dom.server.value.trim());
    setNetStatus('Looking for the room\u2026');
    try {
      await net.join(code, {
        strategy: dom.strategy.value,
        relayUrls: dom.server.value.trim() ? [dom.server.value.trim()] : [],
      });
      history.replaceState(null, '', `#room=${encodeURIComponent(code)}`);
      refreshNetUi();
    } catch (err) {
      setNetStatus(`Could not open the room: ${err.message}`, 'bad');
    }
  });

  dom.becomeBoss.addEventListener('click', () => {
    net.requestBoss();
    dom.becomeBoss.disabled = true;
    setNetStatus('Asking for a boss to drive\u2026', 'live');
  });
}

function remember(key, value) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Private windows can refuse storage; nothing here is worth failing over.
  }
}

// -------------------------------------------------------------------- loop

let lastTime = 0;
let accumulator = 0;
let framesDrawn = 0;
let fpsClock = 0;
let fps = 0;

function frame(now) {
  requestAnimationFrame(frame);
  if (!lastTime) lastTime = now;
  const dt = Math.min(now - lastTime, 250);
  lastTime = now;

  if (online()) {
    // Every peer keeps its own clock and its own copy of the world. A frame
    // is simulated once everybody's buttons for it have arrived, which is
    // what keeps the copies identical.
    net.pump(dt, held | gamepadButtons());
    accumulator = 0;
  } else if (!paused) {
    accumulator += dt;
    let steps = 0;
    while (accumulator >= FRAME_MS && steps < MAX_CATCH_UP) {
      wasm.zelduh_set_input(0, held | gamepadButtons());
      wasm.zelduh_step();
      drainEvents();
      accumulator -= FRAME_MS;
      steps += 1;
    }
    if (steps === MAX_CATCH_UP) accumulator = 0;
  }

  draw();

  framesDrawn += 1;
  fpsClock += dt;
  if (fpsClock >= 500) {
    fps = Math.round((framesDrawn * 1000) / fpsClock);
    framesDrawn = 0;
    fpsClock = 0;
    updateStats();
  }
}

function drainEvents() {
  const len = wasm.zelduh_events_len();
  if (!len) return;
  const data = bytes(wasm.zelduh_events(), len);
  for (let i = 0; i + 1 < data.length; ) {
    const kind = data[i];
    if (kind === 1) {
      audio.play(data[i + 1]);
      i += 2;
    } else if (kind === 5) {
      i += 3;
    } else {
      i += 2;
    }
  }
}

function draw() {
  wasm.zelduh_render(online() && net.slot >= 0 ? net.slot : 0);
  const len = wasm.zelduh_framebuffer_len();
  if (!len) return;
  const src = bytes(wasm.zelduh_framebuffer(), len);
  if (!imageData || imageData.data.length !== len) {
    imageData = ctx.createImageData(dom.canvas.width, dom.canvas.height);
  }
  imageData.data.set(src);
  ctx.putImageData(imageData, 0, 0);
}

function updateStats() {
  if (net) refreshNetUi();
  const want = trackForNow();
  if (audio.playing !== want) audio.setTrack(want);
  const me = online() && net.slot >= 0 ? net.slot : 0;
  const hp = wasm.zelduh_player_health(me);
  dom.stats.textContent =
    `${fps} fps · frame ${wasm.zelduh_frame()} · ` +
    `level ${wasm.zelduh_player_level(me)} · ` +
    `${wasm.zelduh_entity_count()} entities · ` +
    `${wasm.zelduh_tile_count()} tiles · ` +
    `hearts ${(Math.max(hp, 0) / 4).toFixed(2)} · ` +
    `rupees ${wasm.zelduh_player_rupees(me)} · ` +
    `kills ${wasm.zelduh_player_kills(me)}`;
}

// -------------------------------------------------------------------- start

async function main() {
  dom.overlay.hidden = false;
  dom.overlayText.textContent = 'Loading…';
  try {
    wasm = await loadWasm();
  } catch (err) {
    dom.overlayText.textContent =
      `Could not start: ${err.message}. The page needs to be served over http, ` +
      `not opened from the file system.`;
    return;
  }

  dom.canvas.width = wasm.zelduh_screen_width();
  dom.canvas.height = wasm.zelduh_screen_height();
  ctx.imageSmoothingEnabled = false;
  // A phone has the wrong shape for a Game Boy, so by default it gets more
  // of the world instead of black bars. A desktop keeps the original screen
  // unless it is asked for more.
  dom.fill.checked = TOUCH;
  try {
    const saved = localStorage.getItem('zelduh.scale');
    // Default to filling the screen on a touch device, 3x on a desktop.
    dom.scale.value = saved ?? (TOUCH ? '0' : '3');
    const fill = localStorage.getItem('zelduh.fill');
    if (fill !== null) dom.fill.checked = fill === '1';
    const relay = localStorage.getItem('zelduh.server');
    if (relay) dom.server.value = relay;
  } catch {
    dom.scale.value = TOUCH ? '0' : '3';
  }
  if (TOUCH) setupTouchControls();
  applyScreen();
  // Two frames later the layout has settled, including any late web font.
  requestAnimationFrame(() => requestAnimationFrame(applyScreen));

  net = createNet(wasm, {
    restore: (bytes) => !!withBytes(bytes, (ptr, len) => wasm.zelduh_restore(ptr, len)),
    onStepped: drainEvents,
    onState: (message) => refreshNetUi(message),
    onSeed: (seed) => { dom.seed.value = String(seed); },
  });
  setupNetUi();

  // A seed in the address bar makes a world shareable: #seed=1234, and a room
  // code takes you straight into somebody else's: #room=amber-otter-42
  const hash = new URLSearchParams(location.hash.slice(1));
  const fromHash = hash.get('seed');
  if (fromHash) dom.seed.value = fromHash;
  const roomFromHash = hash.get('room');
  dom.room.value = roomFromHash ?? readStored('zelduh.room') ?? randomRoom();
  dom.strategy.value = readStored('zelduh.strategy') ?? 'nostr';
  dom.relayRow.hidden = dom.strategy.value !== 'relay';

  startWorld(parseSeed(dom.seed.value));
  dom.overlay.hidden = true;
  registerServiceWorker();
  window.zelduh = { wasm, startWorld, net, audio, selfId, get held() { return held; } };
  requestAnimationFrame(frame);

  // Arriving on a shared link should just start playing together.
  if (roomFromHash) {
    dom.together.open = true;
    dom.connect.click();
  }
}

/** Reads a remembered setting, or null when storage is not available. */
function readStored(key) {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

/** Registers the offline cache, if the browser has one to offer. */
function registerServiceWorker() {
  if (!('serviceWorker' in navigator)) return;
  // A file:// page has no scope to register against.
  if (location.protocol !== 'https:' && location.hostname !== 'localhost') return;
  navigator.serviceWorker.register('sw.js').catch(() => {
    // Playing without an offline cache is perfectly fine.
  });
}

main();
