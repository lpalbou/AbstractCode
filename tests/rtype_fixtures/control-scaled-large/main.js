// SCALE CONTROL — the control-playable game rendered at 1200x900.
//
// The game is UNCHANGED: same modules, same logic, same virtual-clock
// integration, same everything. The canvas is 2.5x the linear size and a single
// setTransform draws the identical 480x360 logical picture into it. The
// PICTURE is therefore identical up to resolution, and a scale-invariant
// instrument must score this the same as control-playable.
//
// WHY IT EXISTS. Across the corpus R8 reads 0.94 on 160x144-class canvases and
// 0.50 on 800x600-class ones (Welch p=0.0011), and the two highest-scoring arms
// are the two smallest-canvas arms. A correlation cannot say whether that is the
// instrument preferring small canvases or small-canvas games genuinely animating
// more of their screen — the two are perfectly confounded in the corpus. Three
// resolutions of ONE game separate them: if these score differently, the
// confound is in the instrument and every cell threshold in the rubric is
// resolution-dependent.
// POSITIVE CONTROL for the R-Type rubric regression suite.
//
// A small but genuinely playable horizontal shooter: the ship answers the
// arrows, Space fires shots that persist and kill enemies, the stage scrolls by
// itself, and sound is scheduled. It exists so that every adversarial fixture in
// this directory can be required to score BELOW it by a recorded margin.
import { Stage, STAGE_W, STAGE_H } from './stage.js';
import { Ship, Shot, Enemy, Explosion, Pod } from './entities.js';

const canvas = document.getElementById('screen');
const ctx = canvas.getContext('2d');
// Render the 480x360 logical picture into a 1200x900 canvas.
ctx.setTransform(2.5, 0, 0, 2.5, 0, 0);
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

// Held-key map. keydown/keyup, not keypress: the driver holds a key down for a
// run of frames and the game has to see it for every one of them.
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

function hud() {
  ctx.fillStyle = '#dfe9ff';
  ctx.font = '12px monospace';
  ctx.fillText('SCORE ' + score, 8, 14);
  ctx.fillText('POWER ' + ship.power, STAGE_W - 88, 14);
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
  for (const pod of pods) pod.draw(ctx);
  for (const enemy of enemies) enemy.draw(ctx);
  for (const shot of shots) shot.draw(ctx);
  for (const boom of explosions) boom.draw(ctx);
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
