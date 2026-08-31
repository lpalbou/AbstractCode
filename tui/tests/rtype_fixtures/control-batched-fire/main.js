// SECOND POSITIVE CONTROL: identical game to control-playable, batched renderer.
//
// Every shot, enemy, explosion and pod is accumulated into one Path2D per type
// and issued as a SINGLE fill per frame, unconditionally — including when the
// list is empty. So the canvas draw-call rate is exactly constant whether the
// player is firing or not, while the game itself is fully playable.
//
// It exists to test one specific claim in the rubric: that "extra draw calls per
// frame" is a sound signature for a weapon. It is sound for every product in the
// measured corpus, none of which batches; this fixture is the class that rule
// cannot see.
import { Stage, STAGE_W } from './stage.js';
import { Ship, Shot, Enemy, Explosion, Pod } from './entities.js';

const canvas = document.getElementById('screen');
const ctx = canvas.getContext('2d');
const stage = new Stage();
const ship = new Ship(stage.bounds());

const shots = [];
const enemies = [];
const explosions = [];
const pods = [];

const held = Object.create(null);
let cooldown = 0;
let score = 0;
let frame = 0;
let last = -1;
let audio = null;

window.addEventListener('keydown', function (e) {
  held[e.code] = true;
  if (e.code.startsWith('Arrow') || e.code === 'Space') e.preventDefault();
});
window.addEventListener('keyup', function (e) {
  held[e.code] = false;
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

function fire() {
  shots.push(new Shot(ship.x + ship.w, ship.y - 1, ship.power));
  tone(660, 0.05);
}

function spawnWave() {
  const lane = stage.bounds();
  const span = lane.bottom - lane.top;
  enemies.push(new Enemy(lane.top + ((frame * 37) % span), (frame % 6) * 1.1));
  if (frame % 300 === 120) pods.push(new Pod(STAGE_W, lane.top + span / 2));
}

function collide() {
  for (const shot of shots) {
    for (const enemy of enemies) {
      if (!enemy.dead && !shot.dead && enemy.hit(shot)) {
        enemy.dead = true;
        shot.dead = true;
        score += 100;
        explosions.push(new Explosion(enemy.x, enemy.y));
        tone(180, 0.08);
      }
    }
  }
  for (const pod of pods) {
    if (!pod.dead && pod.collected(ship)) {
      pod.dead = true;
      ship.power = Math.min(3, ship.power + 1);
    }
  }
}

function prune(list) {
  for (let i = list.length - 1; i >= 0; i--) {
    if (list[i].dead) list.splice(i, 1);
  }
}

// ONE fill per entity type per frame, whatever the list holds.
function batch(list, color) {
  const path = new Path2D();
  for (const item of list) item.path(path);
  ctx.fillStyle = color;
  ctx.fill(path);
}

function hud() {
  ctx.fillStyle = '#dfe9ff';
  ctx.font = '12px monospace';
  ctx.fillText('SCORE ' + score, 8, 14);
}

function update(dt) {
  const dx = (held.ArrowRight ? 1 : 0) - (held.ArrowLeft ? 1 : 0);
  const dy = (held.ArrowDown ? 1 : 0) - (held.ArrowUp ? 1 : 0);
  ship.move(dx, dy, dt);

  cooldown -= 1;
  if ((held.Space || held.KeyZ) && cooldown <= 0) {
    fire();
    cooldown = 4;
  }

  stage.update(dt);
  for (const shot of shots) shot.update(dt);
  for (const enemy of enemies) enemy.update(dt);
  for (const boom of explosions) boom.update(dt);
  for (const pod of pods) pod.update(dt);
  collide();
  prune(shots);
  prune(enemies);
  prune(explosions);
  prune(pods);

  if (frame % 34 === 0) spawnWave();
  if (frame % 16 === 0) tone(110 + (frame % 4) * 55, 0.12);
}

function render() {
  stage.draw(ctx);
  batch(pods, '#63ff9c');
  batch(enemies, '#c86bff');
  batch(shots, '#ff5f5f');
  batch(explosions, '#ffb03c');
  ship.draw(ctx);
  hud();
}

function loop(now) {
  if (last < 0) last = now;
  const dt = Math.min(0.05, Math.max(0, (now - last) / 1000));
  last = now;
  frame++;
  update(dt);
  render();
  requestAnimationFrame(loop);
}

requestAnimationFrame(loop);
