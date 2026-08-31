// Stage section 3: parallax layer parameters and its scroll bookkeeping.
export const SECTION_3 = {
  index: 3,
  speed: 61,
  layers: 1,
  terrain: 'tileset-3',
};

export class DeltaSection {
  constructor() {
    this.index = SECTION_3.index;
    this.speed = SECTION_3.speed;
    this.layers = SECTION_3.layers;
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

export function makeDelta() {
  const section = new DeltaSection();
  section.describe();
  return section;
}
