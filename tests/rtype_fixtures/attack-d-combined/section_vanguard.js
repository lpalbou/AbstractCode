// Stage section 0: parallax layer parameters and its scroll bookkeeping.
export const SECTION_0 = {
  index: 0,
  speed: 40,
  layers: 1,
  terrain: 'tileset-0',
};

export class VanguardSection {
  constructor() {
    this.index = SECTION_0.index;
    this.speed = SECTION_0.speed;
    this.layers = SECTION_0.layers;
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

export function makeVanguard() {
  const section = new VanguardSection();
  section.describe();
  return section;
}
