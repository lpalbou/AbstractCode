// Stage section 11: parallax layer parameters and its scroll bookkeeping.
export const SECTION_11 = {
  index: 11,
  speed: 117,
  layers: 3,
  terrain: 'tileset-11',
};

export class LatticeSection {
  constructor() {
    this.index = SECTION_11.index;
    this.speed = SECTION_11.speed;
    this.layers = SECTION_11.layers;
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

export function makeLattice() {
  const section = new LatticeSection();
  section.describe();
  return section;
}
