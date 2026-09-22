// The browser side of Zelduh.
//
// Rust owns the world and the pixels; this file owns input, files, sound and
// the canvas. Everything crosses the boundary as integers or as bytes in the
// module's memory, so there is no bindings generator in the build.

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
  sound: document.getElementById('sound'),
  drop: document.getElementById('drop'),
  file: document.getElementById('file'),
  assetStatus: document.getElementById('asset-status'),
  resetAssets: document.getElementById('reset-assets'),
  stats: document.getElementById('stats'),
  touch: document.getElementById('touch'),
};

const ctx = dom.canvas.getContext('2d', { alpha: false });
ctx.imageSmoothingEnabled = false;

let wasm = null;
let held = 0;
let paused = false;
let imageData = null;
const audio = createAudio();

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
  };
}

// ------------------------------------------------------------------- audio

/** A small square-wave synth, in the spirit of the hardware being imitated. */
function createAudio() {
  let context = null;
  // Sfx ids come from the Rust `Sfx` enum, in order.
  const VOICES = [
    { f: 620, to: 300, ms: 70, type: 'square', gain: 0.16 },   // 0 sword swing
    { f: 900, to: 1400, ms: 120, type: 'square', gain: 0.12 }, // 1 sword beam
    { f: 200, to: 90, ms: 90, type: 'square', gain: 0.2 },     // 2 enemy hit
    { f: 320, to: 60, ms: 220, type: 'sawtooth', gain: 0.2 },  // 3 enemy dies
    { f: 180, to: 120, ms: 200, type: 'square', gain: 0.24 },  // 4 hurt
    { f: 300, to: 40, ms: 700, type: 'sawtooth', gain: 0.26 }, // 5 death
    { f: 800, to: 1200, ms: 90, type: 'square', gain: 0.14 },  // 6 pickup
    { f: 1000, to: 1500, ms: 80, type: 'square', gain: 0.12 }, // 7 rupee
    { f: 700, to: 1100, ms: 130, type: 'triangle', gain: 0.16 }, // 8 heart
    { f: 260, to: 260, ms: 60, type: 'square', gain: 0.12 },   // 9 bomb placed
    { f: 140, to: 40, ms: 400, type: 'sawtooth', gain: 0.3 },  // 10 explosion
    { f: 520, to: 900, ms: 80, type: 'square', gain: 0.12 },   // 11 shoot
    { f: 400, to: 800, ms: 200, type: 'triangle', gain: 0.1 }, // 12 boomerang
    { f: 300, to: 500, ms: 160, type: 'square', gain: 0.12 },  // 13 door
    { f: 700, to: 1000, ms: 200, type: 'square', gain: 0.14 }, // 14 unlock
    { f: 500, to: 900, ms: 180, type: 'triangle', gain: 0.14 },// 15 chest
    { f: 900, to: 1600, ms: 320, type: 'triangle', gain: 0.16 },// 16 secret
    { f: 400, to: 700, ms: 90, type: 'square', gain: 0.1 },    // 17 jump
    { f: 250, to: 500, ms: 160, type: 'sine', gain: 0.12 },    // 18 splash
    { f: 600, to: 100, ms: 420, type: 'sine', gain: 0.16 },    // 19 fall
    { f: 380, to: 520, ms: 80, type: 'square', gain: 0.1 },    // 20 lift
    { f: 520, to: 300, ms: 90, type: 'square', gain: 0.12 },   // 21 throw
    { f: 900, to: 700, ms: 60, type: 'square', gain: 0.14 },   // 22 shield
    { f: 640, to: 640, ms: 40, type: 'square', gain: 0.08 },   // 23 text
    { f: 260, to: 160, ms: 140, type: 'sawtooth', gain: 0.22 },// 24 boss hurt
    { f: 220, to: 30, ms: 900, type: 'sawtooth', gain: 0.3 },  // 25 boss dies
    { f: 500, to: 900, ms: 260, type: 'triangle', gain: 0.14 },// 26 stairs
    { f: 160, to: 120, ms: 110, type: 'square', gain: 0.12 },  // 27 error
  ];

  return {
    /** Browsers only allow audio to start from a gesture. */
    unlock() {
      if (!context) {
        const Ctor = window.AudioContext || window.webkitAudioContext;
        if (Ctor) context = new Ctor();
      }
      if (context && context.state === 'suspended') context.resume();
    },
    play(id) {
      if (!context || !dom.sound.checked) return;
      const v = VOICES[id] || VOICES[0];
      const now = context.currentTime;
      const osc = context.createOscillator();
      const gain = context.createGain();
      osc.type = v.type;
      osc.frequency.setValueAtTime(v.f, now);
      osc.frequency.exponentialRampToValueAtTime(Math.max(20, v.to), now + v.ms / 1000);
      gain.gain.setValueAtTime(v.gain, now);
      gain.gain.exponentialRampToValueAtTime(0.0001, now + v.ms / 1000);
      osc.connect(gain).connect(context.destination);
      osc.start(now);
      osc.stop(now + v.ms / 1000 + 0.02);
    },
  };
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

// On-screen controls for touch screens.
if (matchMedia('(pointer: coarse)').matches) {
  dom.touch.hidden = false;
  dom.touch.removeAttribute('aria-hidden');
  for (const el of dom.touch.querySelectorAll('[data-button]')) {
    const name = el.dataset.button;
    const press = (e) => { e.preventDefault(); audio.unlock(); setButton(name, true); };
    const release = (e) => { e.preventDefault(); setButton(name, false); };
    el.addEventListener('pointerdown', press);
    el.addEventListener('pointerup', release);
    el.addEventListener('pointercancel', release);
    el.addEventListener('pointerleave', release);
  }
}

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
  startWorld(parseSeed(dom.seed.value));
});
dom.randomSeed.addEventListener('click', () => {
  const n = Math.floor(Math.random() * 1e9);
  dom.seed.value = String(n);
  audio.unlock();
  startWorld(BigInt(n));
});

function applyScale() {
  const scale = Number(dom.scale.value);
  dom.scaleLabel.textContent = `${scale}x`;
  dom.canvas.style.width = `${dom.canvas.width * scale}px`;
  dom.canvas.style.height = `${dom.canvas.height * scale}px`;
  try {
    localStorage.setItem('zelduh.scale', String(scale));
  } catch {
    // Private windows can refuse storage; the zoom just will not be remembered.
  }
}
dom.scale.addEventListener('input', applyScale);

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

  if (!paused) {
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
  wasm.zelduh_render(0);
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
  const hp = wasm.zelduh_player_health(0);
  dom.stats.textContent =
    `${fps} fps · frame ${wasm.zelduh_frame()} · ` +
    `level ${wasm.zelduh_player_level(0)} · ` +
    `${wasm.zelduh_entity_count()} entities · ` +
    `${wasm.zelduh_tile_count()} tiles · ` +
    `hearts ${(Math.max(hp, 0) / 4).toFixed(2)} · ` +
    `rupees ${wasm.zelduh_player_rupees(0)} · ` +
    `kills ${wasm.zelduh_player_kills(0)}`;
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
  try {
    const saved = localStorage.getItem('zelduh.scale');
    if (saved) dom.scale.value = saved;
  } catch {
    // No stored preference is fine.
  }
  applyScale();

  // A seed in the address bar makes a world shareable: #seed=1234
  const fromHash = new URLSearchParams(location.hash.slice(1)).get('seed');
  if (fromHash) dom.seed.value = fromHash;

  startWorld(parseSeed(dom.seed.value));
  dom.overlay.hidden = true;
  window.zelduh = { wasm, startWorld, get held() { return held; } };
  requestAnimationFrame(frame);
}

main();
