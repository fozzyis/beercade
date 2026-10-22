// ============================================================
// BEERCADE 8-BIT CHIPTUNE BGM ENGINE
// 5 distinct songs with smooth crossfade transitions
// Pure Web Audio API — no external dependencies
// ============================================================

let audioCtx: AudioContext | null = null;
let soundEnabled = true;
let bgmEnabled = false;
let songTimeout: ReturnType<typeof setTimeout> | null = null;
let stepInterval: ReturnType<typeof setInterval> | null = null;
let currentSongIndex = 0;
let currentStep = 0;
let masterGain: GainNode | null = null;

function getAudioContext(): AudioContext | null {
  if (typeof window === 'undefined') return null;
  if (!audioCtx) {
    const AC = window.AudioContext || (window as any).webkitAudioContext;
    if (AC) audioCtx = new AC();
  }
  if (audioCtx?.state === 'suspended') audioCtx.resume();
  return audioCtx;
}

function getMasterGain(): GainNode | null {
  const ctx = getAudioContext();
  if (!ctx) return null;
  if (!masterGain) {
    masterGain = ctx.createGain();
    masterGain.gain.setValueAtTime(0.7, ctx.currentTime);
    masterGain.connect(ctx.destination);
  }
  return masterGain;
}

export function isAudioEnabled() { return soundEnabled; }
export function isBGMEnabled()   { return bgmEnabled; }

export function toggleAudio(enable?: boolean): boolean {
  soundEnabled = enable !== undefined ? enable : !soundEnabled;
  if (!soundEnabled && bgmEnabled) toggleBGM(false);
  return soundEnabled;
}

export function toggleBGM(enable?: boolean): boolean {
  bgmEnabled = enable !== undefined ? enable : !bgmEnabled;
  if (bgmEnabled) { soundEnabled = true; startBGM(); }
  else              stopBGM();
  return bgmEnabled;
}

// ============================================================
// SOUND EFFECTS
// ============================================================

export function playClickSound() {
  if (!soundEnabled) return;
  const ctx = getAudioContext(); if (!ctx) return;
  const osc = ctx.createOscillator(), gain = ctx.createGain();
  osc.type = 'square';
  osc.frequency.setValueAtTime(800, ctx.currentTime);
  osc.frequency.exponentialRampToValueAtTime(300, ctx.currentTime + 0.04);
  gain.gain.setValueAtTime(0.14, ctx.currentTime);
  gain.gain.exponentialRampToValueAtTime(0.01, ctx.currentTime + 0.04);
  osc.connect(gain); gain.connect(ctx.destination);
  osc.start(); osc.stop(ctx.currentTime + 0.04);
}

export function playCoinSound() {
  if (!soundEnabled) return;
  const ctx = getAudioContext(); if (!ctx) return;
  const now = ctx.currentTime;
  const osc = ctx.createOscillator(), gain = ctx.createGain();
  osc.type = 'square';
  osc.frequency.setValueAtTime(987.77, now);
  osc.frequency.setValueAtTime(1318.51, now + 0.08);
  gain.gain.setValueAtTime(0.18, now);
  gain.gain.exponentialRampToValueAtTime(0.01, now + 0.28);
  osc.connect(gain); gain.connect(ctx.destination);
  osc.start(now); osc.stop(now + 0.28);
}

export function playSuccessSound() {
  if (!soundEnabled) return;
  const ctx = getAudioContext(); if (!ctx) return;
  const now = ctx.currentTime;
  [523.25, 659.25, 783.99, 1046.50].forEach((freq, i) => {
    const osc = ctx.createOscillator(), gain = ctx.createGain();
    osc.type = 'square';
    osc.frequency.setValueAtTime(freq, now + i * 0.06);
    gain.gain.setValueAtTime(0.13, now + i * 0.06);
    gain.gain.exponentialRampToValueAtTime(0.01, now + i * 0.06 + 0.08);
    osc.connect(gain); gain.connect(ctx.destination);
    osc.start(now + i * 0.06); osc.stop(now + i * 0.06 + 0.08);
  });
}

// ============================================================
// SONG DEFINITIONS
// Each song: { name, bpm, stepMs, lead[], bass[], arp[] }
// Frequencies in Hz. 0 = rest.
// ============================================================

// Helper: note frequency lookup
function n(note: string): number {
  const map: Record<string, number> = {
    C3:130.81, D3:146.83, E3:164.81, F3:174.61, G3:196.00, A3:220.00, B3:246.94,
    C4:261.63, D4:293.66, E4:329.63, F4:349.23, G4:392.00, A4:440.00, B4:493.88,
    C5:523.25, D5:587.33, E5:659.25, F5:698.46, G5:783.99, A5:880.00, B5:987.77,
    C6:1046.50, D6:1174.66, E6:1318.51, F6:1396.91, G6:1567.98, A6:1760.00,
    Bb3:233.08, Bb4:466.16, Bb5:932.33, Eb4:311.13, Eb5:622.25, Ab4:415.30,
    '0': 0
  };
  return map[note] ?? 0;
}

// ── SONG 1: "COIN DROP" — Classic Mario-style bouncy overworld ──
const SONG_COIN_DROP = {
  name: 'COIN DROP',
  stepMs: 125, // 120 BPM, 16th notes
  lead: [
    // Phrase 1: C major bounce
    'E5','E5','0','E5','0','C5','E5','0', 'G5','0','0','0','G4','0','0','0',
    // Phrase 2: walk up
    'C5','0','0','G4','0','0','E4','0', 'A4','0','B4','0','Bb4','A4','0','0',
    // Phrase 3: run
    'G4','E5','G5','A5','0','F5','G5','0', 'E5','0','C5','D5','B4','0','0','0',
    // Phrase 4: resolve
    'C5','0','G4','0','E4','0','A4','B4', 'Bb4','A4','G4','E5','G5','A5','0','0',
  ].map(n),
  bass: [
    'C3','0','G3','0','C3','0','G3','0', 'C3','0','G3','0','C3','0','G3','0',
    'A3','0','E3','0','A3','0','E3','0', 'G3','0','D3','0','G3','0','D3','0',
    'C3','0','G3','0','C3','0','G3','0', 'C3','0','E3','0','G3','0','C4','0',
    'F3','0','C3','0','G3','0','E3','0', 'C3','G3','C3','G3','C3','G3','C3','0',
  ].map(n),
};

// ── SONG 2: "BOSS BATTLE" — Intense driving minor key ──
const SONG_BOSS_BATTLE = {
  name: 'BOSS BATTLE',
  stepMs: 100, // 150 BPM, urgent
  lead: [
    // Aggressive Street Fighter style
    'E5','0','D5','0','C5','B4','0','0', 'E5','0','D5','E5','G5','0','0','0',
    'A5','0','G5','0','F5','E5','0','0', 'D5','E5','A5','0','0','G5','F5','E5',
    'E5','0','D5','0','C5','D5','E5','0', 'G5','F5','E5','D5','C5','0','0','0',
    'A4','B4','C5','D5','E5','F5','G5','A5','B5','0','0','0','E5','D5','C5','B4',
  ].map(n),
  bass: [
    'A3','0','A3','0','A3','0','G3','0', 'A3','0','A3','0','E3','0','E3','0',
    'F3','0','F3','0','F3','0','E3','0', 'D3','0','D3','0','A2','0','A2','0',
    'A3','0','A3','0','C4','0','B3','0', 'A3','0','G3','0','F3','0','E3','0',
    'D3','0','E3','0','F3','0','G3','0', 'A3','0','A3','A3','A3','A3','A3','0',
  ].map(n),
};

// ── SONG 3: "SPACE INVADERS" — Synthwave arpeggios, cosmic ──
const SONG_SPACE = {
  name: 'SPACE INVADERS',
  stepMs: 110, // 136 BPM
  lead: [
    // Dreamy arpeggio-style melody
    'C5','E5','G5','B4','C5','E5','G5','C6', 'B5','G5','E5','C5','G4','B4','D5','F5',
    'G5','B5','D6','F5','E5','C5','A4','G4', 'F4','A4','C5','E5','G5','A5','B5','G5',
    'C5','E5','G5','E5','C5','G4','E4','G4', 'A4','C5','E5','G5','A5','G5','E5','C5',
    'D5','F5','A5','C6','B5','G5','E5','D5', 'C5','G5','E5','C5','G4','C5','E5','C5',
  ].map(n),
  bass: [
    'C3','0','0','G3','0','0','E3','0', 'G3','0','0','C3','0','0','G3','0',
    'A3','0','0','E3','0','0','C3','0', 'F3','0','0','C3','0','0','G3','0',
    'C3','0','G3','0','C3','0','G3','0', 'A3','0','E3','0','A3','0','E3','0',
    'F3','0','C3','0','F3','0','C3','0', 'G3','0','D3','0','G3','0','C3','0',
  ].map(n),
};

// ── SONG 4: "DOUBLE DRAGON" — Blues-based arcade groove ──
const SONG_DOUBLE_DRAGON = {
  name: 'DOUBLE DRAGON',
  stepMs: 115, // ~130 BPM with swing feel
  lead: [
    // Driving bluesy riff
    'E4','G4','A4','Bb4','A4','G4','E4','0', 'E4','G4','A4','C5','B4','A4','G4','0',
    'A4','C5','D5','Eb5','D5','C5','A4','0', 'G4','A4','C5','E5','D5','C5','B4','0',
    'E5','0','D5','C5','B4','0','A4','G4', 'E5','0','D5','C5','B4','A4','G4','E4',
    'A4','B4','C5','D5','E5','F5','E5','D5', 'C5','B4','A4','G4','E4','0','E4','0',
  ].map(n),
  bass: [
    'A3','A3','0','A3','0','A3','G3','0', 'A3','A3','0','A3','0','C4','B3','0',
    'D3','D3','0','D3','0','D3','C3','0', 'D3','D3','0','D3','0','F3','E3','0',
    'A3','0','A3','0','G3','0','F3','0', 'E3','0','D3','0','C3','0','A3','0',
    'D3','E3','F3','G3','A3','B3','C4','D4', 'E4','0','D4','0','A3','0','A3','0',
  ].map(n),
};

// ── SONG 5: "FINAL LEVEL" — Triumphant upbeat fanfare with chord stabs ──
const SONG_FINAL = {
  name: 'FINAL LEVEL',
  stepMs: 107, // ~140 BPM — energetic
  lead: [
    // Heroic rising melody
    'G4','A4','B4','C5','D5','E5','F5','G5', 'A5','0','G5','F5','E5','D5','C5','B4',
    'C5','D5','E5','F5','G5','A5','B5','C6', 'B5','0','A5','0','G5','0','F5','E5',
    'G5','E5','C5','G5','E5','C5','G5','E5', 'A5','F5','D5','A5','F5','D5','A5','F5',
    'B5','G5','E5','B5','G5','E5','C6','B5', 'A5','G5','F5','E5','D5','C5','G5','C5',
  ].map(n),
  bass: [
    'C3','0','G3','0','C4','0','G3','0', 'F3','0','C4','0','F3','0','C3','0',
    'G3','0','D4','0','G3','0','D4','0', 'C4','0','G3','0','C3','G3','C3','0',
    'C3','E3','G3','C4','E4','G4','C5','E5', 'F3','A3','C4','F4','A4','C5','F5','A5',
    'G3','B3','D4','G4','B4','D5','G5','0', 'C3','G3','E3','C3','G3','C3','G3','C3',
  ].map(n),
};

const SONGS = [
  SONG_COIN_DROP,
  SONG_BOSS_BATTLE,
  SONG_SPACE,
  SONG_DOUBLE_DRAGON,
  SONG_FINAL,
];

// ============================================================
// PLAYBACK ENGINE
// ============================================================

function playNote(
  ctx: AudioContext,
  dest: AudioNode,
  freq: number,
  when: number,
  dur: number,
  type: OscillatorType,
  vol: number
) {
  if (freq === 0) return;
  const osc = ctx.createOscillator();
  const gain = ctx.createGain();
  osc.type = type;
  osc.frequency.setValueAtTime(freq, when);
  gain.gain.setValueAtTime(vol, when);
  gain.gain.exponentialRampToValueAtTime(0.001, when + dur * 0.85);
  osc.connect(gain);
  gain.connect(dest);
  osc.start(when);
  osc.stop(when + dur);
}

function startBGM() {
  stopBGM();

  const ctx = getAudioContext(); if (!ctx) return;
  const mg = getMasterGain(); if (!mg) return;

  const song = SONGS[currentSongIndex % SONGS.length];
  const { stepMs, lead, bass } = song;
  const stepSec = stepMs / 1000;

  currentStep = 0;

  stepInterval = setInterval(() => {
    if (!bgmEnabled) return;
    const ctx2 = getAudioContext(); if (!ctx2) return;
    const mg2 = getMasterGain(); if (!mg2) return;
    const now = ctx2.currentTime;
    const step = currentStep % lead.length;

    // Lead: square wave, thinner volume
    playNote(ctx2, mg2, lead[step],  now, stepSec * 0.85, 'square',   0.09);
    // Bass: triangle wave, softer
    playNote(ctx2, mg2, bass[step],  now, stepSec * 0.8,  'triangle', 0.13);
    // Percussive noise hit on beat 1 & 3 of each bar (every 8 steps)
    if (step % 8 === 0) {
      const noise = ctx2.createOscillator();
      const nGain = ctx2.createGain();
      noise.type = 'sawtooth';
      noise.frequency.setValueAtTime(80 + Math.random() * 20, now);
      nGain.gain.setValueAtTime(0.05, now);
      nGain.gain.exponentialRampToValueAtTime(0.001, now + 0.06);
      noise.connect(nGain); nGain.connect(mg2);
      noise.start(now); noise.stop(now + 0.06);
    }

    currentStep++;

    // Transition to next song after finishing the sequence
    if (currentStep > 0 && currentStep % lead.length === 0) {
      crossfadeToNextSong();
    }
  }, stepMs);
}

function crossfadeToNextSong() {
  const ctx = getAudioContext();
  const mg = getMasterGain();
  if (!ctx || !mg || !bgmEnabled) return;

  // Fade out current song over 1.5s
  mg.gain.setValueAtTime(mg.gain.value, ctx.currentTime);
  mg.gain.linearRampToValueAtTime(0, ctx.currentTime + 1.5);

  songTimeout = setTimeout(() => {
    if (!bgmEnabled) return;
    currentSongIndex = (currentSongIndex + 1) % SONGS.length;
    currentStep = 0;

    // Clear step interval and restart
    if (stepInterval) { clearInterval(stepInterval); stepInterval = null; }

    // Fade back in
    const ctx2 = getAudioContext();
    const mg2 = getMasterGain();
    if (ctx2 && mg2) {
      mg2.gain.setValueAtTime(0, ctx2.currentTime);
      mg2.gain.linearRampToValueAtTime(0.7, ctx2.currentTime + 1.0);
    }

    startBGM();
  }, 1600);
}

function stopBGM() {
  if (stepInterval)  { clearInterval(stepInterval);  stepInterval = null; }
  if (songTimeout)   { clearTimeout(songTimeout);    songTimeout = null;  }

  // Fade master out
  const ctx = getAudioContext();
  const mg = getMasterGain();
  if (ctx && mg) {
    mg.gain.setValueAtTime(mg.gain.value, ctx.currentTime);
    mg.gain.linearRampToValueAtTime(0, ctx.currentTime + 0.3);
    setTimeout(() => {
      if (mg) mg.gain.setValueAtTime(0.7, ctx.currentTime);
    }, 400);
  }
}

export function getCurrentSongName(): string {
  return SONGS[currentSongIndex % SONGS.length]?.name ?? '';
}
