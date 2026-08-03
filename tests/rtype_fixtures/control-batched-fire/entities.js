// Same entities as the plain control, with one difference that matters to the
// rubric: nothing here draws itself. Each entity contributes its rectangles to a
// shared Path2D and the caller issues ONE fill per entity type per frame, so the
// draw-call rate is independent of how many shots, enemies or explosions exist.
//
// This is a normal batching optimisation, and it is invisible to a player. It is
// also the exact case a weapon check that looks only for extra draw calls must
// false-negative on, which is why this fixture is a CONTROL and not an attack:
// the game really is playable, so it must be scored as one.
import { STAGE_W, STAGE_H } from './stage.js';

export class Ship {
  constructor(bounds) {
    this.x = 96;
    this.y = STAGE_H / 2;
    this.w = 22;
    this.h = 12;
    this.speed = 260;
    this.bounds = bounds;
    this.power = 1;
  }

  move(dx, dy, dt) {
    this.x += dx * this.speed * dt;
    this.y += dy * this.speed * dt;
    this.x = Math.max(8, Math.min(STAGE_W - 140, this.x));
    this.y = Math.max(this.bounds.top, Math.min(this.bounds.bottom, this.y));
  }

  draw(ctx) {
    ctx.fillStyle = '#8fd2ff';
    ctx.fillRect(this.x | 0, (this.y - this.h / 2) | 0, this.w, this.h);
    ctx.fillStyle = '#ff9a3c';
    ctx.fillRect((this.x - 5) | 0, (this.y - 2) | 0, 5, 4);
  }
}

export class Shot {
  constructor(x, y, power) {
    this.x = x;
    this.y = y;
    this.speed = 430;
    this.power = power;
    this.dead = false;
  }

  update(dt) {
    this.x += this.speed * dt;
    if (this.x > STAGE_W) this.dead = true;
  }

  path(path) {
    path.rect(this.x | 0, this.y | 0, 9 + this.power * 3, 3);
  }
}

export class Enemy {
  constructor(y, phase) {
    this.x = STAGE_W + 12;
    this.y = y;
    this.baseY = y;
    this.phase = phase;
    this.age = 0;
    this.speed = 92;
    this.dead = false;
  }

  update(dt) {
    this.age += dt;
    this.x -= this.speed * dt;
    this.y = this.baseY + Math.sin(this.age * 3 + this.phase) * 26;
    if (this.x < -20) this.dead = true;
  }

  hit(shot) {
    return shot.x > this.x - 4 && shot.x < this.x + 18 &&
           shot.y > this.y - 12 && shot.y < this.y + 12;
  }

  path(path) {
    path.rect(this.x | 0, (this.y - 8) | 0, 16, 16);
  }
}

export class Explosion {
  constructor(x, y) {
    this.x = x;
    this.y = y;
    this.life = 0.42;
    this.dead = false;
    this.bits = [];
    for (let i = 0; i < 8; i++) {
      const a = (i / 8) * Math.PI * 2;
      this.bits.push({ dx: Math.cos(a) * 78, dy: Math.sin(a) * 78 });
    }
  }

  update(dt) {
    this.life -= dt;
    if (this.life <= 0) this.dead = true;
  }

  path(path) {
    const t = 0.42 - this.life;
    for (const bit of this.bits) {
      path.rect((this.x + bit.dx * t) | 0, (this.y + bit.dy * t) | 0, 3, 3);
    }
  }
}

// Power-up capsule: drifts left, upgrades the weapon when collected.
export class Pod {
  constructor(x, y) {
    this.x = x;
    this.y = y;
    this.dead = false;
  }

  update(dt) {
    this.x -= 60 * dt;
    if (this.x < -12) this.dead = true;
  }

  collected(ship) {
    return Math.abs(this.x - ship.x) < 18 && Math.abs(this.y - ship.y) < 14;
  }

  path(path) {
    path.rect(this.x | 0, this.y | 0, 10, 10);
  }
}
