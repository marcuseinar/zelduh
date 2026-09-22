// Sound: effects, and music.
//
// Two different problems, solved two different ways.
//
// The effects are sfxr's, through the vendored jsfxr. sfxr is the tool that
// made this genre of sound: pick "pickup", "explosion" or "hurt" and it
// throws parameters at a little synthesiser until something lands. That is a
// much better sound than anyone gets by reasoning about oscillators, so the
// presets do the work here and this file only says which preset each event
// wants and nudges the result — pitch, length, loudness — so that a rupee
// rings higher than a heart and a boss dies more heavily than an octorok.
// The random part runs against a fixed seed, so the game sounds the same
// every time it is opened rather than different every session.
//
// The music is written out by hand, because no library has a tune in it.
// It plays through a small Game Boy-shaped voice set: two pulse channels
// with selectable duty, a soft triangle-ish bass, and noise for drums.

const SAMPLE_RATE_HINT = 44100;

// ------------------------------------------------------------------ effects

/**
 * One entry per `Sfx` in the engine, in order.
 *
 * `preset` is the sfxr generator to start from; `over` replaces particular
 * parameters afterwards. The parameter names are sfxr's own.
 */
const EFFECTS = [
  // 0 sword swing: air, not a tone.
  { preset: 'laserShoot', over: { wave_type: 3, p_base_freq: 0.5, p_freq_ramp: -0.25, p_env_sustain: 0.02, p_env_decay: 0.12, p_hpf_freq: 0.25, sound_vol: 0.2 } },
  // 1 sword beam: the sword at full health, flying off.
  { preset: 'laserShoot', over: { wave_type: 0, p_base_freq: 0.5, p_freq_ramp: 0.12, p_duty: 0.35, p_env_decay: 0.25, sound_vol: 0.18 } },
  // 2 enemy hit
  { preset: 'hitHurt', over: { p_base_freq: 0.34, p_env_decay: 0.16, sound_vol: 0.24 } },
  // 3 enemy dies
  { preset: 'explosion', over: { p_base_freq: 0.26, p_env_decay: 0.32, sound_vol: 0.24 } },
  // 4 the hero is hurt: lower and rougher than hitting something.
  { preset: 'hitHurt', over: { wave_type: 1, p_base_freq: 0.2, p_freq_ramp: -0.14, p_env_decay: 0.3, sound_vol: 0.3 } },
  // 5 the hero dies: the long one.
  { preset: 'explosion', over: { p_base_freq: 0.18, p_env_sustain: 0.2, p_env_decay: 0.7, p_lpf_freq: 0.5, sound_vol: 0.3 } },
  // 6 pickup
  { preset: 'pickupCoin', over: { p_base_freq: 0.52, p_arp_mod: 0.38, p_arp_speed: 0.62, p_env_decay: 0.26, sound_vol: 0.2 } },
  // 7 rupee: the same shape, brighter.
  { preset: 'pickupCoin', over: { p_base_freq: 0.68, p_arp_mod: 0.42, p_arp_speed: 0.55, p_env_decay: 0.22, sound_vol: 0.18 } },
  // 8 heart: warmer, no arpeggio.
  { preset: 'pickupCoin', over: { wave_type: 2, p_base_freq: 0.44, p_arp_mod: 0.5, p_arp_speed: 0.7, p_env_decay: 0.34, sound_vol: 0.2 } },
  // 9 a bomb going down
  { preset: 'blipSelect', over: { wave_type: 0, p_base_freq: 0.24, p_env_decay: 0.1, p_duty: 0.2, sound_vol: 0.18 } },
  // 10 and going off
  { preset: 'explosion', over: { p_base_freq: 0.14, p_env_sustain: 0.18, p_env_decay: 0.55, sound_vol: 0.34 } },
  // 11 something is shot at you
  { preset: 'laserShoot', over: { wave_type: 0, p_base_freq: 0.42, p_freq_ramp: -0.18, p_env_decay: 0.16, sound_vol: 0.16 } },
  // 12 boomerang: the wobble is the point.
  { preset: 'laserShoot', over: { wave_type: 2, p_base_freq: 0.36, p_freq_ramp: 0, p_vib_strength: 0.5, p_vib_speed: 0.7, p_env_sustain: 0.18, p_env_decay: 0.2, sound_vol: 0.14 } },
  // 13 a door
  { preset: 'powerUp', over: { wave_type: 0, p_base_freq: 0.24, p_freq_ramp: 0.14, p_env_decay: 0.3, p_duty: 0.5, sound_vol: 0.16 } },
  // 14 a key turning
  { preset: 'powerUp', over: { p_base_freq: 0.4, p_freq_ramp: 0.2, p_env_decay: 0.36, sound_vol: 0.18 } },
  // 15 a chest
  { preset: 'powerUp', over: { wave_type: 2, p_base_freq: 0.34, p_freq_ramp: 0.16, p_env_decay: 0.42, sound_vol: 0.2 } },
  // 16 a secret found: the fanfare.
  { preset: 'powerUp', over: { wave_type: 0, p_base_freq: 0.42, p_arp_mod: 0.58, p_arp_speed: 0.66, p_env_sustain: 0.18, p_env_decay: 0.6, p_duty: 0.35, sound_vol: 0.22 } },
  // 17 jump
  { preset: 'jump', over: { p_base_freq: 0.32, p_freq_ramp: 0.2, p_env_decay: 0.18, sound_vol: 0.14 } },
  // 18 into the water
  { preset: 'explosion', over: { p_base_freq: 0.3, p_freq_ramp: -0.1, p_env_decay: 0.28, p_lpf_freq: 0.32, sound_vol: 0.2 } },
  // 19 down a hole
  { preset: 'powerUp', over: { wave_type: 2, p_base_freq: 0.6, p_freq_ramp: -0.32, p_env_sustain: 0.2, p_env_decay: 0.4, sound_vol: 0.22 } },
  // 20 lifting something
  { preset: 'blipSelect', over: { wave_type: 0, p_base_freq: 0.36, p_freq_ramp: 0.1, p_env_decay: 0.1, sound_vol: 0.14 } },
  // 21 throwing it
  { preset: 'laserShoot', over: { wave_type: 3, p_base_freq: 0.42, p_freq_ramp: -0.2, p_env_decay: 0.12, p_hpf_freq: 0.2, sound_vol: 0.16 } },
  // 22 turned on a shield
  { preset: 'hitHurt', over: { wave_type: 0, p_base_freq: 0.62, p_env_decay: 0.1, p_duty: 0.1, sound_vol: 0.16 } },
  // 23 a letter of text
  { preset: 'blipSelect', over: { wave_type: 0, p_base_freq: 0.5, p_env_decay: 0.05, p_duty: 0.5, sound_vol: 0.08 } },
  // 24 a boss is hurt: heavier than a monster.
  { preset: 'hitHurt', over: { wave_type: 1, p_base_freq: 0.16, p_env_decay: 0.34, sound_vol: 0.3 } },
  // 25 and a boss dying, the longest sound in the game.
  { preset: 'explosion', over: { p_base_freq: 0.12, p_env_sustain: 0.3, p_env_decay: 0.9, sound_vol: 0.36 } },
  // 26 stairs
  { preset: 'powerUp', over: { wave_type: 0, p_base_freq: 0.3, p_freq_ramp: 0.1, p_env_decay: 0.44, p_duty: 0.4, sound_vol: 0.16 } },
  // 27 that did not work
  { preset: 'blipSelect', over: { wave_type: 1, p_base_freq: 0.18, p_env_decay: 0.12, sound_vol: 0.16 } },
];

/**
 * A generator that always produces the same numbers.
 *
 * sfxr's presets reach for `Math.random`, which is exactly what makes them
 * good — they explore a range that sounds right — and exactly what would
 * make the game sound different every time it was opened. Running them
 * against a fixed sequence keeps the happy accident and throws away the
 * inconsistency.
 */
function seeded(seed) {
  let state = seed >>> 0;
  return () => {
    // xorshift32: small, and good enough to pick synthesiser parameters.
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    state >>>= 0;
    return state / 4294967296;
  };
}

/** Builds every effect's parameters, the same way every time. */
function buildEffects(jsfxr) {
  const real = Math.random;
  const fake = seeded(0x5eed_1e55);
  Math.random = fake;
  try {
    return EFFECTS.map(({ preset, over }) => {
      const params = jsfxr.sfxr.generate(preset);
      Object.assign(params, over);
      return params;
    });
  } finally {
    Math.random = real;
  }
}

// -------------------------------------------------------------------- music

/** Semitone offsets within an octave, for parsing note names. */
const STEPS = { c: 0, d: 2, e: 4, f: 5, g: 7, a: 9, b: 11 };

/**
 * Turns `f#4` into a frequency in Hz, or 0 for a rest.
 *
 * Note names are lower case, sharps only, with the octave last, so middle C
 * is `c4` and the A above it is `a4` at 440Hz.
 */
function noteHz(name) {
  if (name === 'r') return 0;
  const m = /^([a-g])(#?)(\d)$/.exec(name);
  if (!m) return 0;
  const midi = (Number(m[3]) + 1) * 12 + STEPS[m[1]] + (m[2] ? 1 : 0);
  return 440 * 2 ** ((midi - 69) / 12);
}

/**
 * Reads a part.
 *
 * A part is tokens separated by spaces, each `note:sixteenths`, with the
 * length defaulting to two — an eighth note — because most of them are.
 * `r` is a rest. Drum parts use `x` for a kick, `o` for a snare and `h` for
 * a hat.
 */
function parsePart(text) {
  const out = [];
  let at = 0;
  for (const token of text.trim().split(/\s+/)) {
    if (!token) continue;
    const [name, length] = token.split(':');
    const steps = Number(length ?? 2);
    out.push({ at, steps, name });
    at += steps;
  }
  return { steps: at, notes: out };
}

/**
 * The tunes.
 *
 * Original, and written to sit under a game rather than in front of one:
 * a short loop, a clear shape, and nothing that draws attention to itself
 * on the fortieth time round.
 */
const TUNES = {
  // Out in the world: D major, walking pace, going somewhere.
  overworld: {
    bpm: 132,
    lead: `
      a4:2 d5:2 e5:2 f#5:4 e5:2 d5:2 b4:2
      a4:2 d5:2 e5:2 f#5:4 g5:2 f#5:2 e5:2
      d5:2 e5:2 f#5:2 g5:2 a5:4 f#5:4
      e5:2 f#5:2 g5:2 e5:2 d5:8
      f#5:2 g5:2 a5:4 b5:2 a5:2 f#5:4
      g5:2 f#5:2 e5:2 d5:2 c#5:4 e5:4
      d5:2 c#5:2 b4:2 c#5:2 d5:4 f#5:4
      e5:4 c#5:4 d5:8
    `,
    harmony: `
      r:16
      r:16
      r:16
      r:16
      d5:2 e5:2 f#5:4 g5:2 f#5:2 d5:4
      b4:2 a4:2 g4:2 f#4:2 a4:4 c#5:4
      a4:2 g4:2 f#4:2 a4:2 b4:4 d5:4
      c#5:4 a4:4 f#4:8
    `,
    bass: `
      d3:2 r:2 a3:2 r:2 d3:2 r:2 a3:2 r:2
      g3:2 r:2 d4:2 r:2 g3:2 r:2 b3:2 r:2
      d3:2 r:2 a3:2 r:2 d3:2 r:2 f#3:2 r:2
      a3:2 r:2 e3:2 r:2 a3:2 r:2 a3:2 r:2
      d3:2 r:2 a3:2 r:2 d3:2 r:2 a3:2 r:2
      g3:2 r:2 d4:2 r:2 a3:2 r:2 e3:2 r:2
      b3:2 r:2 f#3:2 r:2 g3:2 r:2 b3:2 r:2
      a3:2 r:2 a3:2 r:2 d3:4 d3:4
    `,
    drums: `
      x:4 o:4 x:2 x:2 o:4
      x:4 o:4 x:2 x:2 o:4
      x:4 o:4 x:2 x:2 o:4
      x:4 o:4 x:4 o:2 o:2
      x:4 o:4 x:2 x:2 o:4
      x:4 o:4 x:2 x:2 o:4
      x:4 o:4 x:2 x:2 o:4
      x:4 o:4 x:4 o:2 o:2
    `,
  },
  // Underground: D minor, slower, and mostly quiet.
  dungeon: {
    bpm: 96,
    lead: `
      d5:4 r:4 f5:2 e5:2 d5:4
      c5:4 r:4 e5:2 d5:2 c5:4
      a#4:4 r:4 d5:2 c5:2 a#4:4
      a4:8 r:8
      d5:4 r:4 f5:2 g5:2 a5:4
      g5:4 f5:4 e5:8
      f5:2 e5:2 d5:2 c5:2 a#4:4 c5:4
      a4:8 r:8
    `,
    harmony: `
      r:16
      r:16
      r:16
      r:16
      a4:4 r:4 c5:2 d5:2 f5:4
      e5:4 d5:4 c5:8
      d5:2 c5:2 a#4:2 a4:2 f4:4 a4:4
      f4:8 r:8
    `,
    bass: `
      d3:8 d3:8
      c3:8 c3:8
      a#2:8 a#2:8
      a2:8 a2:4 e3:4
      d3:8 d3:8
      c3:8 c3:8
      a#2:8 f3:8
      a2:8 a2:8
    `,
    drums: `
      x:8 r:8
      x:8 r:8
      x:8 r:8
      x:8 o:8
      x:8 r:8
      x:8 r:8
      x:8 r:8
      x:8 o:4 o:4
    `,
  },
  // A boss: the same key, twice the speed, no room to breathe.
  boss: {
    bpm: 168,
    lead: `
      d5:1 d5:1 d5:2 f5:2 e5:2 d5:2 c5:2 d5:4
      d5:1 d5:1 d5:2 g5:2 f5:2 e5:2 d5:2 a#4:4
      c5:1 c5:1 c5:2 e5:2 d5:2 c5:2 a#4:2 c5:4
      a4:2 a#4:2 c5:2 d5:2 e5:4 f5:4
      d5:1 d5:1 d5:2 f5:2 e5:2 d5:2 c5:2 d5:4
      f5:2 e5:2 d5:2 c5:2 a#4:4 a4:4
      a#4:2 c5:2 d5:2 e5:2 f5:4 a5:4
      g5:2 f5:2 e5:2 d5:2 d5:8
    `,
    harmony: `
      a4:1 a4:1 a4:2 c5:2 a#4:2 a4:2 g4:2 a4:4
      a4:1 a4:1 a4:2 a#4:2 a4:2 g4:2 f4:2 f4:4
      g4:1 g4:1 g4:2 a4:2 g4:2 f4:2 e4:2 g4:4
      f4:2 g4:2 a4:2 a#4:2 c5:4 d5:4
      a4:1 a4:1 a4:2 c5:2 a#4:2 a4:2 g4:2 a4:4
      a4:2 a#4:2 a4:2 g4:2 f4:4 e4:4
      f4:2 g4:2 a4:2 c5:2 d5:4 f5:4
      e5:2 d5:2 c5:2 a#4:2 a4:8
    `,
    bass: `
      d3:1 d3:1 d3:2 d3:1 d3:1 d3:2 d3:1 d3:1 d3:2 d3:1 d3:1 d3:2
      d3:1 d3:1 d3:2 d3:1 d3:1 d3:2 a#2:1 a#2:1 a#2:2 a#2:1 a#2:1 a#2:2
      c3:1 c3:1 c3:2 c3:1 c3:1 c3:2 c3:1 c3:1 c3:2 c3:1 c3:1 c3:2
      f3:1 f3:1 f3:2 f3:1 f3:1 f3:2 a2:1 a2:1 a2:2 a2:1 a2:1 a2:2
      d3:1 d3:1 d3:2 d3:1 d3:1 d3:2 d3:1 d3:1 d3:2 d3:1 d3:1 d3:2
      a#2:1 a#2:1 a#2:2 a#2:1 a#2:1 a#2:2 a2:1 a2:1 a2:2 a2:1 a2:1 a2:2
      a#2:1 a#2:1 a#2:2 a#2:1 a#2:1 a#2:2 d3:1 d3:1 d3:2 d3:1 d3:1 d3:2
      a3:1 a3:1 a3:2 a3:1 a3:1 a3:2 d3:8
    `,
    drums: `
      x:2 h:2 o:2 h:2 x:2 h:2 o:2 h:2
      x:2 h:2 o:2 h:2 x:2 h:2 o:2 h:2
      x:2 h:2 o:2 h:2 x:2 h:2 o:2 h:2
      x:2 h:2 o:2 h:2 x:2 x:2 o:2 o:2
      x:2 h:2 o:2 h:2 x:2 h:2 o:2 h:2
      x:2 h:2 o:2 h:2 x:2 h:2 o:2 h:2
      x:2 h:2 o:2 h:2 x:2 h:2 o:2 h:2
      x:2 h:2 o:2 h:2 o:2 o:2 o:2 o:2
    `,
  },
};

/** How far ahead of the clock notes are handed to the audio hardware. */
const SCHEDULE_AHEAD = 0.2;
/** How often the scheduler wakes up, in milliseconds. */
const SCHEDULE_EVERY = 40;

/** A pulse wave of the given duty, as a Fourier series. */
function pulseWave(context, duty, harmonics = 24) {
  const real = new Float32Array(harmonics + 1);
  const imag = new Float32Array(harmonics + 1);
  for (let n = 1; n <= harmonics; n += 1) {
    // The Fourier series of a pulse: each harmonic's amplitude follows the
    // sine of the duty, which is what gives 12.5% its thin, reedy sound and
    // 50% the flat buzz of a square.
    imag[n] = (2 / (n * Math.PI)) * Math.sin(Math.PI * n * duty);
  }
  return context.createPeriodicWave(real, imag, { disableNormalization: false });
}

/** A second of white noise, reused by every drum. */
function noiseBuffer(context) {
  const length = Math.floor(context.sampleRate * 0.5);
  const buffer = context.createBuffer(1, length, context.sampleRate);
  const data = buffer.getChannelData(0);
  const rand = seeded(0x1234_5678);
  for (let i = 0; i < length; i += 1) data[i] = rand() * 2 - 1;
  return buffer;
}

// ---------------------------------------------------------------- the whole

export function createSound({ wantsEffects, wantsMusic }) {
  let context = null;
  let buffers = null;
  let master = null;
  let musicGain = null;
  let waves = null;
  let noise = null;

  let track = null;
  let tune = null;
  let step = 0;
  let stepAt = 0;
  let timer = null;

  function ensure() {
    if (context) return true;
    const Ctor = window.AudioContext || window.webkitAudioContext;
    if (!Ctor) return false;
    context = new Ctor({ sampleRate: SAMPLE_RATE_HINT });
    master = context.createGain();
    master.gain.value = 0.9;
    master.connect(context.destination);
    musicGain = context.createGain();
    musicGain.gain.value = 0;
    musicGain.connect(master);
    waves = {
      lead: pulseWave(context, 0.25),
      harmony: pulseWave(context, 0.125),
      bass: pulseWave(context, 0.5),
    };
    noise = noiseBuffer(context);
    if (window.jsfxr) {
      const params = buildEffects(window.jsfxr);
      buffers = params.map((p) => {
        try {
          return window.jsfxr.sfxr.toWebAudio(p, context).buffer;
        } catch {
          return null;
        }
      });
    }
    return true;
  }

  /** Plays one note on one of the pulse voices. */
  function voice(name, hz, when, seconds, gain) {
    const osc = context.createOscillator();
    osc.setPeriodicWave(waves[name] ?? waves.lead);
    osc.frequency.setValueAtTime(hz, when);
    const envelope = context.createGain();
    // A short attack stops every note starting with a click, and a release
    // that never quite reaches zero keeps the exponential ramp legal.
    envelope.gain.setValueAtTime(0.0001, when);
    envelope.gain.exponentialRampToValueAtTime(gain, when + 0.008);
    envelope.gain.setValueAtTime(gain, when + seconds * 0.7);
    envelope.gain.exponentialRampToValueAtTime(0.0001, when + seconds);
    osc.connect(envelope).connect(musicGain);
    osc.start(when);
    osc.stop(when + seconds + 0.02);
  }

  /** Plays one drum: a burst of noise, shaped into a kick, snare or hat. */
  function drum(kind, when) {
    if (kind === 'r') return;
    const source = context.createBufferSource();
    source.buffer = noise;
    const filter = context.createBiquadFilter();
    const envelope = context.createGain();
    const shape = {
      x: { type: 'lowpass', hz: 220, length: 0.13, gain: 0.5 },
      o: { type: 'bandpass', hz: 1800, length: 0.1, gain: 0.3 },
      h: { type: 'highpass', hz: 6000, length: 0.04, gain: 0.14 },
    }[kind] ?? { type: 'bandpass', hz: 1000, length: 0.08, gain: 0.2 };
    filter.type = shape.type;
    filter.frequency.setValueAtTime(shape.hz, when);
    envelope.gain.setValueAtTime(shape.gain, when);
    envelope.gain.exponentialRampToValueAtTime(0.0001, when + shape.length);
    source.connect(filter).connect(envelope).connect(musicGain);
    source.start(when);
    source.stop(when + shape.length + 0.02);
  }

  /** Hands the next stretch of the tune to the audio hardware. */
  function schedule() {
    if (!tune || !context) return;
    const seconds = 60 / tune.bpm / 4;
    while (stepAt < context.currentTime + SCHEDULE_AHEAD) {
      for (const [name, part] of Object.entries(tune.parts)) {
        for (const note of part.notes) {
          if (note.at !== step) continue;
          const length = note.steps * seconds;
          if (name === 'drums') {
            drum(note.name, stepAt);
          } else {
            const hz = noteHz(note.name);
            if (hz) voice(name, hz, stepAt, length * 0.92, name === 'bass' ? 0.1 : 0.075);
          }
        }
      }
      step = (step + 1) % tune.steps;
      stepAt += seconds;
    }
  }

  function stopMusic() {
    if (timer !== null) clearInterval(timer);
    timer = null;
    tune = null;
    if (musicGain) musicGain.gain.value = 0;
  }

  return {
    /** Browsers only allow audio to start from a gesture. */
    unlock() {
      if (!ensure()) return;
      if (context.state === 'suspended') context.resume();
      if (track && !tune) this.setTrack(track);
    },

    play(id) {
      if (!context || !wantsEffects() || !buffers) return;
      const buffer = buffers[id] ?? buffers[0];
      if (!buffer) return;
      const source = context.createBufferSource();
      source.buffer = buffer;
      source.connect(master);
      source.start();
    },

    /** Switches to one of the tunes, or to silence with `null`. */
    setTrack(name) {
      track = name;
      if (!context || !wantsMusic() || !name || !TUNES[name]) {
        stopMusic();
        return;
      }
      const source = TUNES[name];
      const parts = {};
      let steps = 0;
      for (const part of ['lead', 'harmony', 'bass', 'drums']) {
        if (!source[part]) continue;
        parts[part] = parsePart(source[part]);
        steps = Math.max(steps, parts[part].steps);
      }
      const already = tune;
      tune = { bpm: source.bpm, parts, steps };
      if (!already) {
        step = 0;
        stepAt = context.currentTime + 0.1;
      }
      musicGain.gain.cancelScheduledValues(context.currentTime);
      musicGain.gain.setValueAtTime(Math.max(musicGain.gain.value, 0.0001), context.currentTime);
      musicGain.gain.linearRampToValueAtTime(0.5, context.currentTime + 0.5);
      if (timer === null) timer = setInterval(schedule, SCHEDULE_EVERY);
      schedule();
    },

    /** Called when the music setting is turned off. */
    refresh() {
      if (!wantsMusic()) stopMusic();
      else if (track && !tune) this.setTrack(track);
    },

    get playing() {
      return track;
    },

    /** The audio context, once there is one. Used by the page's own tests. */
    get context() {
      return context;
    },

    /** Attaches a node to the output, so a test can measure what comes out. */
    tap(node) {
      if (master) master.connect(node);
    },
  };
}
