// Stage section 7: parallax layer parameters and its scroll bookkeeping.
export const SECTION_7 = {
  index: 7,
  speed: 89,
  layers: 2,
  terrain: 'tileset-7',
};

export class HalcyonSection {
  constructor() {
    this.index = SECTION_7.index;
    this.speed = SECTION_7.speed;
    this.layers = SECTION_7.layers;
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

export function makeHalcyon() {
  const section = new HalcyonSection();
  section.describe();
  return section;
}
