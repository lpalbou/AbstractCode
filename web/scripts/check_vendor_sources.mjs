#!/usr/bin/env node
/**
 * The vendored AbstractUIC snapshots under src/vendor/ match the kit this app installs
 * (`npm run build`, so CI and the release job run it too).
 *
 * Each snapshot's provenance.json records the sha256 of every kit file it was built from
 * (`sources`, paths inside the owning package). This fails (exit 1) unless every one of them is
 * byte-identical to the same file of the installed package in node_modules, which
 * `npm run check:lock` pins to the npm registry. So a release never ships a snapshot of an
 * unpublished kit commit under a lock that names a published version.
 *
 * After a relock, regenerate the snapshots from an AbstractUIC checkout at the published commit
 * (`npm view @abstractframework/panel-chat@<version> gitHead`): `npm run sync:conversation`,
 * `npm run sync:sessions`, `npm run sync:docs`.
 *
 *   node scripts/check_vendor_sources.mjs [--dir <app dir>]
 */
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const args = process.argv.slice(2);
const dirArg = args.indexOf("--dir");
const appDir = dirArg >= 0 ? resolve(args[dirArg + 1]) : resolve(dirname(fileURLToPath(import.meta.url)), "..");
const vendorDir = join(appDir, "src", "vendor");
const SCOPE = "@abstractframework/";

const pkg = JSON.parse(readFileSync(join(appDir, "package.json"), "utf8"));
const declared = new Set([...Object.keys(pkg.dependencies || {}), ...Object.keys(pkg.devDependencies || {})]);
const hash = (path) => createHash("sha256").update(readFileSync(path)).digest("hex");
const problems = [];
const summary = [];

const snapshots = existsSync(vendorDir)
  ? readdirSync(vendorDir).filter((name) => existsSync(join(vendorDir, name, "provenance.json")))
  : [];
if (snapshots.length === 0) problems.push(`no vendored snapshot with a provenance.json under ${vendorDir}`);

for (const name of snapshots) {
  const manifest = JSON.parse(readFileSync(join(vendorDir, name, "provenance.json"), "utf8"));
  const owner = String(manifest.owner || "");
  const sources = Object.entries(manifest.sources || {});
  if (!owner.startsWith(SCOPE) || !declared.has(owner)) {
    problems.push(`${name}: owner ${JSON.stringify(owner)} is not an ${SCOPE}* package this app declares`);
    continue;
  }
  if (sources.length === 0) {
    problems.push(`${name}: provenance.json records no sources, so nothing ties it to ${owner}`);
    continue;
  }
  const pkgDir = join(appDir, "node_modules", ...owner.split("/"));
  const pkgJson = join(pkgDir, "package.json");
  if (!existsSync(pkgJson)) {
    problems.push(`${name}: ${owner} is not installed (run npm ci)`);
    continue;
  }
  const version = JSON.parse(readFileSync(pkgJson, "utf8")).version;
  let differ = 0;
  for (const [file, expected] of sources) {
    const path = resolve(pkgDir, file);
    if (!path.startsWith(pkgDir + sep)) {
      problems.push(`${name}: source path ${JSON.stringify(file)} leaves the package`);
      differ++;
    } else if (!existsSync(path)) {
      problems.push(`${name}: ${file} is not in the installed ${owner} ${version}`);
      differ++;
    } else if (hash(path) !== expected) {
      problems.push(`${name}: ${file} differs from the installed ${owner} ${version}`);
      differ++;
    }
  }
  if (!differ) summary.push(`${name} = ${owner.slice(SCOPE.length)} ${version} (${sources.length} files)`);
}

if (problems.length) {
  console.error(`check_vendor_sources: FAILED (${appDir})`);
  for (const p of problems.slice(0, 20)) console.error(`  - ${p}`);
  if (problems.length > 20) console.error(`  - … and ${problems.length - 20} more`);
  console.error("  Regenerate the snapshots from the published kit commit (see the header of this script).");
  process.exit(1);
}
console.log(`check_vendor_sources: OK — ${summary.join("; ")}`);
