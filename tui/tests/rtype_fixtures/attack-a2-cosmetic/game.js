// ATTACK (a'): ATTRACT LOOP WITH A COSMETIC, HUD-ONLY KEY RESPONSE.
//
// Identical scripted animation to attack (a) — nothing the player does can move
// the ship, aim a shot or kill an enemy. The only difference is that a keypress
// now lights an indicator in the HUD corner and prints the key's name there.
//
// This is the escalation of (a) against every "did the picture change" test:
//   * the frame DOES change on a keypress, so a pixel/hash differential fires;
//   * the change is in the SAME PLACE whichever key is pressed, so a check that
//     requires the response to be DISPLACED along the axis being tested must
//     reject it;
//   * the change DECAYS in 12 frames, so a check that requires the input to have
//     consequences that outlive the keypress must reject it;
//   * and it costs extra draw calls while it is lit, which is the signature a
//     draw-rate-only weapon check looks for.
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

// THE ATTACK. The key is read, and it is spent entirely on decoration: an
// indicator in the corner of the HUD, lit for exactly as long as the key is
// HELD and showing its name. No game state is touched.
//
// Lit-while-held, not a decaying flash: the driver holds a key for a run of
// frames and samples the canvas at the end of that run, so a flash that decays
// in 12 frames is invisible at the checkpoint and this fixture would test
// nothing. MEASURED: with a 12-frame decay, the ArrowUp and ArrowDown branches
// came back pixel-identical (0 cells) and this attack scored exactly what the
// empty-handler one did.
let heldKey = '';
window.addEventListener('keydown', function (e) {
  heldKey = e.code;
});
window.addEventListener('keyup', function (e) {
  heldKey = '';
});

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
  drawKeyLamp();
}

// The whole of this page's "interactivity": a lamp and a label, in one fixed
// corner, lit while a key is down. It costs three extra draw calls a frame for
// every frame the key is held, which is the signature a draw-rate-only weapon
// check reads as "a projectile was rendered".
function drawKeyLamp() {
  if (!heldKey) return;
  ctx.fillStyle = '#ffd23c';
  ctx.fillRect(W - 26, 22, 14, 14);
  ctx.fillStyle = '#7a5a00';
  ctx.fillRect(W - 23, 25, 8, 8);
  ctx.fillStyle = '#ffd23c';
  ctx.fillText(heldKey, W - 130, 34);
}

function loop(now) {
  if (last < 0) last = now;
  const dt = Math.min(0.05, Math.max(0, (now - last) / 1000));
  last = now;
  update(dt);
  render();
  requestAnimationFrame(loop);
}

requestAnimationFrame(loop);
