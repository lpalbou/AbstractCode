// Stage section 10: parallax layer parameters and its scroll bookkeeping.
export const SECTION_10 = {
  index: 10,
  speed: 110,
  layers: 2,
  terrain: 'tileset-10',
};

export class KeystoneSection {
  constructor() {
    this.index = SECTION_10.index;
    this.speed = SECTION_10.speed;
    this.layers = SECTION_10.layers;
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

export function makeKeystone() {
  const section = new KeystoneSection();
  section.describe();
  return section;
}
