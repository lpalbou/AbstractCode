// Stage section 12: parallax layer parameters and its scroll bookkeeping.
export const SECTION_12 = {
  index: 12,
  speed: 124,
  layers: 1,
  terrain: 'tileset-12',
};

export class MeridianSection {
  constructor() {
    this.index = SECTION_12.index;
    this.speed = SECTION_12.speed;
    this.layers = SECTION_12.layers;
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

export function makeMeridian() {
  const section = new MeridianSection();
  section.describe();
  return section;
}
