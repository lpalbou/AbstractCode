// Stage section 9: parallax layer parameters and its scroll bookkeeping.
export const SECTION_9 = {
  index: 9,
  speed: 103,
  layers: 1,
  terrain: 'tileset-9',
};

export class JunctionSection {
  constructor() {
    this.index = SECTION_9.index;
    this.speed = SECTION_9.speed;
    this.layers = SECTION_9.layers;
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

export function makeJunction() {
  const section = new JunctionSection();
  section.describe();
  return section;
}
