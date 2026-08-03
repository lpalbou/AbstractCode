// ATTACK (c): MODULARITY BOUGHT WITH A SHELL SCRIPT.
//
// Thirteen files, each one export const plus one two-line function. No loop, no
// input, no entity: the page paints one string and stops. The file TREE looks
// like a well-factored codebase, and every file in it is padding.
//
// HISTORICAL: 13 files of exactly this shape scored 4.75/5 on modularity and
// 27.75/30 on the whole source tier — higher than 23 of the 24 real products.
import { step0 } from './mod0.js';
import { step1 } from './mod1.js';
import { step2 } from './mod2.js';
import { step3 } from './mod3.js';
import { step4 } from './mod4.js';
import { step5 } from './mod5.js';
import { step6 } from './mod6.js';
import { step7 } from './mod7.js';
import { step8 } from './mod8.js';
import { step9 } from './mod9.js';
import { step10 } from './mod10.js';
import { step11 } from './mod11.js';
import { step12 } from './mod12.js';

let total = 0;
total = step0(total);
total = step1(total);
total = step2(total);
total = step3(total);
total = step4(total);
total = step5(total);
total = step6(total);
total = step7(total);
total = step8(total);
total = step9(total);
total = step10(total);
total = step11(total);
total = step12(total);

const ctx = document.getElementById('screen').getContext('2d');
ctx.fillStyle = '#04060f';
ctx.fillRect(0, 0, 480, 360);
ctx.fillStyle = '#8fd2ff';
ctx.font = '28px monospace';
ctx.fillText('R-TYPE ' + total, 130, 190);
