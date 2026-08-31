// Stage section 5: parallax layer parameters and its scroll bookkeeping.
export const SECTION_5 = {
  index: 5,
  speed: 75,
  layers: 3,
  terrain: 'tileset-5',
};

export class FissureSection {
  constructor() {
    this.index = SECTION_5.index;
    this.speed = SECTION_5.speed;
    this.layers = SECTION_5.layers;
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

export function makeFissure() {
  const section = new FissureSection();
  section.describe();
  return section;
}
