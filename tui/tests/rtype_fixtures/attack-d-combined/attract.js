// ATTACK (d), part 1: the attract loop from attack (a), unchanged in substance.
// The keydown and keyup handlers are empty; nothing a player does reaches a
// pixel. Everything else on this page exists to buy source-tier points.
const canvas = document.getElementById('screen');
const ctx = canvas.getContext('2d');
const W = 480;
const H = 360;
const TERRAIN_H = 42;
const STRIPE = 24;

const stars = [];
for (let i = 0; i < 72; i++) {
  stars.push({
    x: Math.random() * W,
    y: Math.random() * H,
    speed: 46 + (i % 2) * 88,
    size: 1 + (i % 2),
  });
}

const shots = [];
const enemies = [];
const explosions = [];
let scroll = 0;
let frame = 0;
let last = -1;
let score = 0;
let audio = null;

// THE ATTACK. Listeners are registered — a probe that only counts listeners sees
// a well-behaved game — and the handlers do nothing at all.
window.addEventListener('keydown', function (e) {});
window.addEventListener('keyup', function (e) {});

function tone(freq, dur) {
  try {
    if (!audio) audio = new (window.AudioContext || window.webkitAudioContext)();
    const osc = audio.createOscillator();
    const gain = audio.createGain();
    osc.frequency.value = freq;
    gain.gain.value = 0.04;
    osc.connect(gain).connect(audio.destination);
    osc.start();
    osc.stop(audio.currentTime + dur);
  } catch (err) {
    audio = null;
  }
}

// The "player" is on rails.
function shipX() { return 96 + Math.sin(frame / 47) * 34; }
function shipY() { return H / 2 + Math.sin(frame / 23) * 82; }

function update(dt) {
  frame++;
  scroll += 74 * dt;
  for (const star of stars) {
    star.x -= star.speed * dt;
    if (star.x < 0) star.x += W;
  }
  if (frame % 11 === 0) {
    shots.push({ x: shipX() + 22, y: shipY(), dead: false });
    tone(660, 0.05);
  }
  if (frame % 34 === 0) {
    enemies.push({ x: W + 12, y: 46 + ((frame * 37) % 268), age: 0, dead: false });
  }
  for (const shot of shots) {
    shot.x += 430 * dt;
    if (shot.x > W) shot.dead = true;
  }
  for (const enemy of enemies) {
    enemy.age += dt;
    enemy.x -= 92 * dt;
    if (enemy.x < -20) enemy.dead = true;
    for (const shot of shots) {
      if (!enemy.dead && !shot.dead && Math.abs(shot.x - enemy.x) < 12 &&
          Math.abs(shot.y - enemy.y) < 12) {
        enemy.dead = true;
        shot.dead = true;
        score += 100;
        explosions.push({ x: enemy.x, y: enemy.y, life: 0.42, dead: false });
        tone(180, 0.08);
      }
    }
  }
  for (const boom of explosions) {
    boom.life -= dt;
    if (boom.life <= 0) boom.dead = true;
  }
  prune(shots);
  prune(enemies);
  prune(explosions);
  if (frame % 16 === 0) tone(110 + (frame % 4) * 55, 0.12);
}

function prune(list) {
  for (let i = list.length - 1; i >= 0; i--) {
    if (list[i].dead) list.splice(i, 1);
  }
}

function drawTerrain() {
  const offset = scroll % STRIPE;
  for (let x = -STRIPE; x < W + STRIPE; x += STRIPE) {
    const px = (x - offset) | 0;
    ctx.fillStyle = '#27472a';
    ctx.fillRect(px, 0, STRIPE - 5, TERRAIN_H);
    ctx.fillStyle = '#38663c';
    ctx.fillRect(px, H - TERRAIN_H, STRIPE - 5, TERRAIN_H);
  }
}

function render() {
  ctx.fillStyle = '#04060f';
  ctx.fillRect(0, 0, W, H);
  for (const star of stars) {
    ctx.fillStyle = star.speed > 100 ? '#e6ecff' : '#5d6ea0';
    ctx.fillRect(star.x | 0, star.y | 0, star.size, star.size);
  }
  drawTerrain();
  for (const enemy of enemies) {
    ctx.fillStyle = '#c86bff';
    ctx.fillRect(enemy.x | 0, (enemy.y - 8) | 0, 16, 16);
    ctx.fillStyle = '#2a0033';
    ctx.fillRect((enemy.x + 2) | 0, (enemy.y - 3) | 0, 5, 5);
  }
  for (const shot of shots) {
    ctx.fillStyle = '#ff5f5f';
    ctx.fillRect(shot.x | 0, shot.y | 0, 9, 3);
  }
  for (const boom of explosions) {
    const t = 0.42 - boom.life;
    ctx.fillStyle = boom.life > 0.2 ? '#fff3b0' : '#ff7a2f';
    for (let i = 0; i < 8; i++) {
      const a = (i / 8) * Math.PI * 2;
      ctx.fillRect((boom.x + Math.cos(a) * 78 * t) | 0,
                   (boom.y + Math.sin(a) * 78 * t) | 0, 3, 3);
    }
  }
  ctx.fillStyle = '#8fd2ff';
  ctx.fillRect(shipX() | 0, (shipY() - 6) | 0, 22, 12);
  ctx.fillStyle = '#ffffff';
  ctx.fillRect((shipX() + 16) | 0, (shipY() - 2) | 0, 8, 4);
  ctx.fillStyle = '#dfe9ff';
  ctx.font = '12px monospace';
  ctx.fillText('SCORE ' + score, 8, 14);
  ctx.fillText('ATTRACT MODE', W - 110, 14);
}

export function loop(now) {
  if (last < 0) last = now;
  const dt = Math.min(0.05, Math.max(0, (now - last) / 1000));
  last = now;
  update(dt);
  render();
  requestAnimationFrame(loop);
}

