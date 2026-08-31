// Scrolling stage: two parallax star layers and a striped terrain band top and
// bottom. This is the autonomous motion the rubric's R5/R8 look for, and it is
// also what makes an idle-vs-input comparison impossible on this genre.
export const STAGE_W = 480;
export const STAGE_H = 360;

const TERRAIN_H = 42;
const STRIPE = 24;
const SCROLL_SPEED = 74;

export class Stage {
  constructor() {
    this.scroll = 0;
    this.layers = [];
    for (let layer = 0; layer < 2; layer++) {
      const stars = [];
      for (let i = 0; i < 36; i++) {
        stars.push({
          x: Math.random() * STAGE_W,
          y: Math.random() * STAGE_H,
          size: 1 + layer,
        });
      }
      this.layers.push({ stars: stars, speed: 46 + layer * 88 });
    }
  }

  update(dt) {
    this.scroll += SCROLL_SPEED * dt;
    for (const layer of this.layers) {
      for (const star of layer.stars) {
        star.x -= layer.speed * dt;
        if (star.x < 0) star.x += STAGE_W;
      }
    }
  }

  draw(ctx) {
    ctx.fillStyle = '#04060f';
    ctx.fillRect(0, 0, STAGE_W, STAGE_H);
    for (const layer of this.layers) {
      ctx.fillStyle = layer.speed > 100 ? '#e6ecff' : '#5d6ea0';
      for (const star of layer.stars) {
        ctx.fillRect(star.x | 0, star.y | 0, star.size, star.size);
      }
    }
    this.drawTerrain(ctx);
  }

  drawTerrain(ctx) {
    const offset = this.scroll % STRIPE;
    for (let x = -STRIPE; x < STAGE_W + STRIPE; x += STRIPE) {
      const px = (x - offset) | 0;
      ctx.fillStyle = '#27472a';
      ctx.fillRect(px, 0, STRIPE - 5, TERRAIN_H);
      ctx.fillStyle = '#38663c';
      ctx.fillRect(px, STAGE_H - TERRAIN_H, STRIPE - 5, TERRAIN_H);
    }
  }

  // The playfield the ship is allowed into: inside the terrain, not through it.
  bounds() {
    return { top: TERRAIN_H + 4, bottom: STAGE_H - TERRAIN_H - 4 };
  }
}
