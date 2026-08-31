// Stage section 8: parallax layer parameters and its scroll bookkeeping.
export const SECTION_8 = {
  index: 8,
  speed: 96,
  layers: 3,
  terrain: 'tileset-8',
};

export class IngressSection {
  constructor() {
    this.index = SECTION_8.index;
    this.speed = SECTION_8.speed;
    this.layers = SECTION_8.layers;
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

export function makeIngress() {
  const section = new IngressSection();
  section.describe();
  return section;
}
