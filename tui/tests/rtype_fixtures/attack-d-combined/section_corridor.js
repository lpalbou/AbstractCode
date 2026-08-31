// Stage section 2: parallax layer parameters and its scroll bookkeeping.
export const SECTION_2 = {
  index: 2,
  speed: 54,
  layers: 3,
  terrain: 'tileset-2',
};

export class CorridorSection {
  constructor() {
    this.index = SECTION_2.index;
    this.speed = SECTION_2.speed;
    this.layers = SECTION_2.layers;
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

export function makeCorridor() {
  const section = new CorridorSection();
  section.describe();
  return section;
}
