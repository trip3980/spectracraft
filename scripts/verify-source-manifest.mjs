#!/usr/bin/env node
/**
 * Deterministic source-integrity manifest generator and verifier (Audit M17).
 *
 * Matches `spectracraft_mono::migration::source_inventory(Path::new("spectracraft-mono"))`
 * byte-for-byte: hashes all files under `spectracraft-mono/src`, `spectracraft-mono/tests`,
 * the crate docs/metadata files, and the workspace `Cargo.lock` using sorted keys and
 * forward-slash relative paths.
 *
 * Usage:
 *   node scripts/verify-source-manifest.mjs          # Verify against spectracraft-mono/SOURCE-SHA256.json
 *   node scripts/verify-source-manifest.mjs --write  # Regenerate spectracraft-mono/SOURCE-SHA256.json
 */

import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const repoRoot = path.resolve(__dirname, '..');
const crateRoot = path.join(repoRoot, 'spectracraft-mono');
const manifestPath = path.join(crateRoot, 'SOURCE-SHA256.json');

function sha256File(filePath) {
  const bytes = fs.readFileSync(filePath);
  return crypto.createHash('sha256').update(bytes).digest('hex');
}

function walkFiles(dirPath, out) {
  const entries = fs.readdirSync(dirPath, { withFileTypes: true });
  for (const entry of entries) {
    const full = path.join(dirPath, entry.name);
    if (entry.isSymbolicLink()) {
      throw new Error(`Refusing ambiguous symlink in source inventory: ${full}`);
    }
    if (entry.isDirectory()) {
      walkFiles(full, out);
    } else if (entry.isFile()) {
      out.push(full);
    }
  }
}

function buildSourceInventory() {
  const files = [];
  for (const folder of ['src', 'tests']) {
    walkFiles(path.join(crateRoot, folder), files);
  }
  for (const name of [
    'Cargo.toml',
    'README.md',
    'NATIVE-GUITAR.md',
    'REPRODUCE.md',
    'THIRD-PARTY.md',
    'MIGRATION.tsv',
    'RETIRED-DELETIONS.tsv',
  ]) {
    const full = path.join(crateRoot, name);
    if (!fs.existsSync(full)) {
      throw new Error(`Expected tracked file missing: ${full}`);
    }
    files.push(full);
  }

  const map = new Map();
  for (const full of files) {
    const rel = path.relative(crateRoot, full).split(path.sep).join('/');
    map.set(rel, sha256File(full));
  }

  const crateLock = path.join(crateRoot, 'Cargo.lock');
  const workspaceLock = path.join(repoRoot, 'Cargo.lock');
  if (fs.existsSync(crateLock)) {
    map.set('Cargo.lock', sha256File(crateLock));
  } else if (fs.existsSync(workspaceLock)) {
    map.set('Cargo.lock', sha256File(workspaceLock));
  } else {
    throw new Error('Cargo.lock not found in crate or workspace root');
  }

  const sorted = {};
  for (const key of Array.from(map.keys()).sort()) {
    sorted[key] = map.get(key);
  }
  return sorted;
}

const writeMode = process.argv.includes('--write');
const actual = buildSourceInventory();

if (writeMode) {
  fs.writeFileSync(manifestPath, `${JSON.stringify(actual, null, 2)}\n`, 'utf8');
  console.log(`Updated ${path.relative(repoRoot, manifestPath)} (${Object.keys(actual).length} entries).`);
  process.exit(0);
}

if (!fs.existsSync(manifestPath)) {
  console.error(`Manifest missing: ${path.relative(repoRoot, manifestPath)}`);
  process.exit(1);
}

const expected = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
const diffs = [];
const allKeys = new Set([...Object.keys(expected), ...Object.keys(actual)]);
for (const key of Array.from(allKeys).sort()) {
  if (!(key in expected)) {
    diffs.push(`+ ${key} (untracked in manifest)`);
  } else if (!(key in actual)) {
    diffs.push(`- ${key} (missing from workspace)`);
  } else if (expected[key] !== actual[key]) {
    diffs.push(`~ ${key} (hash mismatch)`);
  }
}

if (diffs.length > 0) {
  console.error('SOURCE-SHA256.json is stale or mismatched:');
  for (const line of diffs) {
    console.error(`  ${line}`);
  }
  console.error('Run `npm run update:manifest` after intentional Rust source changes.');
  process.exit(1);
}

console.log(`All ${Object.keys(actual).length} source fingerprints in spectracraft-mono/SOURCE-SHA256.json match.`);
