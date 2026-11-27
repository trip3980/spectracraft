import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {
  ArtifactLeaseManager,
  acquireSessionsWorkspace,
  checkMaintenanceAuth,
  checkRemoteAccessAuth,
  decodeUploadDisplayName,
  isAllowedMutationOrigin,
  isLoopbackHost,
  mapExpressErrorToJson,
  measureDirectoryUsage,
  resolveBindHost,
  resolveStrictRunArtifact,
} from './serverSecurity';

test('H9: resolveBindHost defaults to loopback 127.0.0.1 and honors --host / SPECTRACRAFT_HOST', () => {
  assert.equal(resolveBindHost([], {}), '127.0.0.1');
  assert.equal(isLoopbackHost('127.0.0.1'), true);
  assert.equal(isLoopbackHost('localhost'), true);
  assert.equal(isLoopbackHost('::1'), true);
  assert.equal(isLoopbackHost('0.0.0.0'), false);

  assert.equal(resolveBindHost([], { SPECTRACRAFT_HOST: '0.0.0.0' }), '0.0.0.0');
  assert.equal(resolveBindHost(['--host', '192.168.1.5'], { SPECTRACRAFT_HOST: '0.0.0.0' }), '192.168.1.5');
  assert.equal(resolveBindHost(['--host=0.0.0.0'], {}), '0.0.0.0');
});

test('H9: isAllowedMutationOrigin enforces full scheme+host and ignores untrusted X-Forwarded-Host', () => {
  // Same-origin loopback allowed
  assert.equal(
    isAllowedMutationOrigin('http://localhost:3000', 'localhost:3000', undefined, 'http', {}).allowed,
    true,
  );
  assert.equal(
    isAllowedMutationOrigin('http://127.0.0.1:3000', 'localhost:3000', undefined, 'http', {}).allowed,
    true,
  );

  // Scheme mismatch blocked
  assert.equal(
    isAllowedMutationOrigin('https://localhost:3000', 'localhost:3000', undefined, 'http', {}).allowed,
    false,
  );

  // Different port on same hostname blocked
  assert.equal(
    isAllowedMutationOrigin('http://localhost:9999', 'localhost:3000', undefined, 'http', {}).allowed,
    false,
  );

  // Spoofed X-Forwarded-Host ignored when SPECTRACRAFT_TRUST_PROXY is unset
  assert.equal(
    isAllowedMutationOrigin('https://evil.example', 'localhost:3000', 'evil.example', 'http', {}).allowed,
    false,
  );

  // Trusted proxy allows matching forwarded host
  assert.equal(
    isAllowedMutationOrigin('https://studio.example', 'localhost:3000', 'studio.example', 'http', {
      SPECTRACRAFT_TRUST_PROXY: '1',
    }).allowed,
    true,
  );

  // Explicit SPECTRACRAFT_ALLOWED_ORIGINS allowlist
  assert.equal(
    isAllowedMutationOrigin('https://app.spectracraft.local:8443', 'localhost:3000', undefined, 'http', {
      SPECTRACRAFT_ALLOWED_ORIGINS: 'https://app.spectracraft.local:8443',
    }).allowed,
    true,
  );
});

test('H9: checkRemoteAccessAuth enforces Bearer token when remote or non-loopback', () => {
  // Local loopback with no token configured is allowed
  assert.equal(checkRemoteAccessAuth(undefined, '127.0.0.1', {}, '127.0.0.1').allowed, true);

  // Non-loopback in production without token fails closed (503) even for loopback client
  const prodMisconfigured = checkRemoteAccessAuth(
    undefined,
    '0.0.0.0',
    { NODE_ENV: 'production' },
    '127.0.0.1',
  );
  assert.equal(prodMisconfigured.allowed, false);
  assert.equal(prodMisconfigured.status, 503);
  assert.equal(prodMisconfigured.code, 'AUTH_MISCONFIGURED');

  // Non-loopback in development blocks direct remote client IP without token (503)
  const remoteClient = checkRemoteAccessAuth(
    undefined,
    '0.0.0.0',
    { NODE_ENV: 'development' },
    '192.168.1.42',
  );
  assert.equal(remoteClient.allowed, false);
  assert.equal(remoteClient.status, 503);

  // Non-loopback in development allows local loopback client when remote mode is off
  assert.equal(
    checkRemoteAccessAuth(undefined, '0.0.0.0', { NODE_ENV: 'development' }, '::ffff:127.0.0.1').allowed,
    true,
  );

  // Remote mode rejects missing or wrong token (401)
  const missing = checkRemoteAccessAuth(undefined, '127.0.0.1', {
    SPECTRACRAFT_REMOTE_MODE: '1',
    SPECTRACRAFT_AUTH_TOKEN: 'secret-token-123',
  }, '127.0.0.1');
  assert.equal(missing.allowed, false);
  assert.equal(missing.status, 401);

  const wrong = checkRemoteAccessAuth('Bearer wrong-token', '0.0.0.0', {
    SPECTRACRAFT_AUTH_TOKEN: 'secret-token-123',
  }, '127.0.0.1');
  assert.equal(wrong.allowed, false);
  assert.equal(wrong.status, 401);

  // Valid Bearer token succeeds
  const valid = checkRemoteAccessAuth('Bearer secret-token-123', '0.0.0.0', {
    SPECTRACRAFT_AUTH_TOKEN: 'secret-token-123',
  }, '192.168.1.42');
  assert.equal(valid.allowed, true);
});

test('H9: checkMaintenanceAuth disables /api/updates/action in production and enforces admin token', () => {
  const prodBlocked = checkMaintenanceAuth('/api/updates/action', undefined, {
    NODE_ENV: 'production',
  });
  assert.equal(prodBlocked.allowed, false);
  assert.equal(prodBlocked.status, 403);
  assert.equal(prodBlocked.code, 'UPDATES_DISABLED_IN_PRODUCTION');

  // Explicit dev opt-in still respects admin token
  const needsAdmin = checkMaintenanceAuth('/api/updates/action', undefined, {
    NODE_ENV: 'production',
    SPECTRACRAFT_ENABLE_DEV_UPDATES: '1',
    SPECTRACRAFT_ADMIN_TOKEN: 'admin-xyz',
  });
  assert.equal(needsAdmin.allowed, false);
  assert.equal(needsAdmin.status, 403);
  assert.equal(needsAdmin.code, 'ADMIN_TOKEN_REQUIRED');

  const okAdmin = checkMaintenanceAuth('/api/updates/action', 'Bearer admin-xyz', {
    NODE_ENV: 'production',
    SPECTRACRAFT_ENABLE_DEV_UPDATES: '1',
    SPECTRACRAFT_ADMIN_TOKEN: 'admin-xyz',
  });
  assert.equal(okAdmin.allowed, true);
});

test('H10: acquireSessionsWorkspace protects live server workspace and reclaims stale dead-PID lock', () => {
  const baseRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'sc-h10-test-'));
  try {
    // First server acquires baseRoot exclusively
    const lease1 = acquireSessionsWorkspace(baseRoot, 1001, (p) => p === 1001);
    assert.equal(lease1.isolated, false);
    assert.equal(lease1.canPruneBase, true);
    assert.equal(lease1.sessionsRoot, baseRoot);

    const activeFile = path.join(baseRoot, 'active-session.wav');
    fs.writeFileSync(activeFile, 'active-audio');

    // Second concurrent server sees PID 1001 is alive -> isolates into per-PID dir, cannot prune base
    const lease2 = acquireSessionsWorkspace(baseRoot, 2002, (p) => p === 1001 || p === 2002);
    assert.equal(lease2.isolated, true);
    assert.equal(lease2.canPruneBase, false);
    assert.notEqual(lease2.sessionsRoot, baseRoot);
    assert.equal(fs.existsSync(activeFile), true, 'second server must not touch first server files');
    lease2.release();

    // Simulate crash of PID 1001 (lock left behind, PID no longer alive) -> PID 3003 reclaims baseRoot
    const lease3 = acquireSessionsWorkspace(baseRoot, 3003, (p) => p === 3003);
    assert.equal(lease3.isolated, false);
    assert.equal(lease3.canPruneBase, true);
    assert.equal(lease3.sessionsRoot, baseRoot);
    lease3.release();
    assert.equal(fs.existsSync(path.join(baseRoot, '.server-owner.lock')), false);
  } finally {
    fs.rmSync(baseRoot, { recursive: true, force: true });
  }
});

test('H11: ArtifactLeaseManager tracks overlapping leases and protects parent directories', () => {
  const mgr = new ArtifactLeaseManager();
  const dir = '/tmp/sc-session-dir';
  const file = path.join(dir, 'run-1', 'take.wav');

  assert.equal(mgr.isLeased(file), false);
  assert.equal(mgr.hasLeaseUnder(dir), false);

  const releaseA = mgr.acquire(file);
  const releaseB = mgr.acquire(file);
  assert.equal(mgr.isLeased(file), true);
  assert.equal(mgr.hasLeaseUnder(dir), true);

  // Double-releasing A must not decrement B's lease
  releaseA();
  releaseA();
  assert.equal(mgr.isLeased(file), true);
  assert.equal(mgr.hasLeaseUnder(dir), true);

  releaseB();
  assert.equal(mgr.isLeased(file), false);
  assert.equal(mgr.hasLeaseUnder(dir), false);
});

test('H5: resolveStrictRunArtifact returns 404/410 for missing/evicted run IDs without silent baseline fallback', () => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'sc-h5-test-'));
  try {
    const baseline = path.join(tmpDir, 'baseline.wav');
    const run1 = path.join(tmpDir, 'run1.wav');
    fs.writeFileSync(baseline, 'base');
    fs.writeFileSync(run1, 'r1');

    const map = new Map<string, string>([['run-1', run1]]);
    const evicted = new Set<string>(['run-evicted']);

    // Empty selector resolves baseline explicitly
    const baseRes = resolveStrictRunArtifact('', map, evicted, baseline, 'dry take');
    assert.equal(baseRes.ok, true);
    if (baseRes.ok) assert.equal(baseRes.isBaseline, true);

    // Known active run resolves exact artifact
    const foundRes = resolveStrictRunArtifact('run-1', map, evicted, baseline, 'dry take');
    assert.equal(foundRes.ok, true);
    if (foundRes.ok) assert.equal(foundRes.filePath, run1);

    // Unknown run returns 404 ARTIFACT_NOT_FOUND — never baseline
    const missingRes = resolveStrictRunArtifact('nonexistent-id', map, evicted, baseline, 'dry take');
    assert.equal(missingRes.ok, false);
    if (!missingRes.ok) {
      assert.equal(missingRes.status, 404);
      assert.equal(missingRes.code, 'ARTIFACT_NOT_FOUND');
    }

    // Evicted run returns 410 ARTIFACT_EXPIRED — never baseline
    const expiredRes = resolveStrictRunArtifact('run-evicted', map, evicted, baseline, 'dry take');
    assert.equal(expiredRes.ok, false);
    if (!expiredRes.ok) {
      assert.equal(expiredRes.status, 410);
      assert.equal(expiredRes.code, 'ARTIFACT_EXPIRED');
    }
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

test('M6: decodeUploadDisplayName decodes UTF-8 X-Filename-Encoded and sanitizes path separators', () => {
  const jp = 'ギター_日本語_take #1.wav';
  const name = decodeUploadDisplayName({
    'x-filename-encoded': encodeURIComponent(jp),
    'x-filename': 'fallback.wav',
  });
  assert.equal(name, jp);

  // Path traversal in header is stripped to basename
  const traversal = decodeUploadDisplayName({
    'x-filename-encoded': encodeURIComponent('../../etc/café_solo.wav'),
  });
  assert.equal(traversal, 'café_solo.wav');

  // Malformed percent-encoding falls back to ASCII X-Filename
  const fallback = decodeUploadDisplayName({
    'x-filename-encoded': '%E0%A4%A',
    'x-filename': 'safe_take.wav',
  });
  assert.equal(fallback, 'safe_take.wav');
});

test('M8: measureDirectoryUsage accurately sums recursive file bytes and counts', () => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'sc-m8-test-'));
  try {
    fs.writeFileSync(path.join(tmpDir, 'a.bin'), Buffer.alloc(1200));
    fs.mkdirSync(path.join(tmpDir, 'sub'));
    fs.writeFileSync(path.join(tmpDir, 'sub', 'b.bin'), Buffer.alloc(345));

    const usage = measureDirectoryUsage(tmpDir);
    assert.equal(usage.files, 2);
    assert.equal(usage.bytes, 1545);
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

test('M14: mapExpressErrorToJson returns structured JSON for 400, 413, and strips paths from 500', () => {
  const badJson = mapExpressErrorToJson({ type: 'entity.parse.failed', status: 400 });
  assert.equal(badJson.status, 400);
  assert.equal(badJson.body.code, 'INVALID_JSON');

  const tooLarge = mapExpressErrorToJson({ type: 'entity.too.large', status: 413 });
  assert.equal(tooLarge.status, 413);
  assert.equal(tooLarge.body.code, 'PAYLOAD_TOO_LARGE');

  const internal = mapExpressErrorToJson(
    new Error('Crash at /app/applet/server.ts while reading /tmp/secret/a.wav'),
    ['/app/applet', '/tmp/secret'],
  );
  assert.equal(internal.status, 500);
  assert.equal(internal.body.code, 'INTERNAL_ERROR');
  assert.ok(!internal.body.error.includes('/app/applet'));
  assert.ok(!internal.body.error.includes('/tmp/secret'));
});
