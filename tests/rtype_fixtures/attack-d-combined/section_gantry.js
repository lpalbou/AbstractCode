// Stage section 6: parallax layer parameters and its scroll bookkeeping.
export const SECTION_6 = {
  index: 6,
  speed: 82,
  layers: 1,
  terrain: 'tileset-6',
};

export class GantrySection {
  constructor() {
    this.index = SECTION_6.index;
    this.speed = SECTION_6.speed;
    this.layers = SECTION_6.layers;
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

export function makeGantry() {
  const section = new GantrySection();
  section.describe();
  return section;
}
