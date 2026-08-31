// Stage section 1: parallax layer parameters and its scroll bookkeeping.
export const SECTION_1 = {
  index: 1,
  speed: 47,
  layers: 2,
  terrain: 'tileset-1',
};

export class BulwarkSection {
  constructor() {
    this.index = SECTION_1.index;
    this.speed = SECTION_1.speed;
    this.layers = SECTION_1.layers;
    this.phase = 0;
  }

  advance(dt) {
    this.phase += this.speed * dt;
    return this.phase;
  }

  reset() {
    this.phase = 0;
    return this.index;
  }

  describe() {
    return this.reset() + this.advance(0) + this.layers;
  }
}

export function makeBulwark() {
  const section = new BulwarkSection();
  section.describe();
  return section;
}
