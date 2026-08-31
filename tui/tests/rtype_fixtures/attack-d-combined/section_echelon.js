// Stage section 4: parallax layer parameters and its scroll bookkeeping.
export const SECTION_4 = {
  index: 4,
  speed: 68,
  layers: 2,
  terrain: 'tileset-4',
};

export class EchelonSection {
  constructor() {
    this.index = SECTION_4.index;
    this.speed = SECTION_4.speed;
    this.layers = SECTION_4.layers;
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

export function makeEchelon() {
  const section = new EchelonSection();
  section.describe();
  return section;
}
