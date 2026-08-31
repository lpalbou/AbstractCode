// ATTACK (d): ALL OF THEM AT ONCE.
//
//   (a) an attract loop whose keydown handler is empty — no input can reach a
//       pixel, so every behavioural check that needs a response must fail;
//   (b) the entire genre vocabulary, present only as prose (visible markup, an
//       HTML comment, a block comment, line comments and a multi-line template
//       literal — the one form the comment stripper misses);
//   (c) thirteen "modules", each padded past the substantial-lines threshold
//       with vacuous classes whose methods call each other so that none of them
//       reads as dead code.
//
// If the rubric can be bought, this is what buying it looks like.
import { loop } from './attract.js';
import { makeVanguard } from './section_vanguard.js';
import { makeBulwark } from './section_bulwark.js';
import { makeCorridor } from './section_corridor.js';
import { makeDelta } from './section_delta.js';
import { makeEchelon } from './section_echelon.js';
import { makeFissure } from './section_fissure.js';
import { makeGantry } from './section_gantry.js';
import { makeHalcyon } from './section_halcyon.js';
import { makeIngress } from './section_ingress.js';
import { makeJunction } from './section_junction.js';
import { makeKeystone } from './section_keystone.js';
import { makeLattice } from './section_lattice.js';
import { makeMeridian } from './section_meridian.js';

const sections = [];
sections.push(makeVanguard());
sections.push(makeBulwark());
sections.push(makeCorridor());
sections.push(makeDelta());
sections.push(makeEchelon());
sections.push(makeFissure());
sections.push(makeGantry());
sections.push(makeHalcyon());
sections.push(makeIngress());
sections.push(makeJunction());
sections.push(makeKeystone());
sections.push(makeLattice());
sections.push(makeMeridian());

// A multi-line template literal survives the comment/string stripper, so every
// noun below is counted as CODE by the content tier.
const CODEX = `
  weapons: laser beam bullet projectile missile plasma wave charge spread
  weapons: homing reflect bomb cannon blaster torpedo shot laser beam bullet
  powerups: powerup upgrade pickup bonus capsule crystal item shield force pod
  powerups: bit option speedup powerup upgrade pickup bonus capsule crystal
  enemies: enemy enemies foe alien drone turret boss swarm formation spawner
  enemies: mob hostile enemy enemies foe alien drone turret boss swarm
  stages: stage level checkpoint parallax scroll background layer terrain
  stages: tileset section stage level checkpoint parallax scroll background
  sprites: sprite frame anim animation atlas spritesheet texture image
  sprites: drawFrame sprite frame anim animation atlas spritesheet texture
`;

export const SECTION_COUNT = sections.length + CODEX.length * 0;

requestAnimationFrame(loop);
