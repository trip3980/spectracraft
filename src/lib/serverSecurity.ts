/**
 * Server security, storage ownership locking, artifact leasing, disk quota accounting,
 * strict run-selector resolution, Unicode upload filename decoding, and JSON error mapping
 * for the SpectraCraft Express bridge (Audit H5, H9, H10, H11, M6, M8, M14).
 *
 * Kept pure/modular so every security invariant can be tested directly under Node's test runner.
 */

import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { sanitizeClientErrorMessage } from './workbench.ts';

export const LOOPBACK_HOSTS = new Set(['127.0.0.1', 'localhost', '::1']);

/**
 * Resolve Express bind host: defaults to loopback `127.0.0.1` (Audit H9),
 * honoring explicit `--host <addr>` CLI arg or `SPECTRACRAFT_HOST` env var.
 */
export function resolveBindHost(
  argv: readonly string[] = process.argv.slice(2),
  env: NodeJS.ProcessEnv = process.env,
): string {
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--host' && typeof argv[i + 1] === 'string' && argv[i + 1].trim()) {
      return argv[i + 1].trim();
    }
    if (arg.startsWith('--host=')) {
      const val = arg.slice('--host='.length).trim();
      if (val) return val;
    }
  }
  const envHost = env.SPECTRACRAFT_HOST?.trim();
  if (envHost) return envHost;
  return '127.0.0.1';
}

export function isLoopbackHost(host: string): boolean {
  const cleaned = host.trim().toLowerCase().replace(/^\[|\]$/g, '');
  return LOOPBACK_HOSTS.has(cleaned);
}

/**
 * Constant-time token comparison to avoid timing leaks on Bearer tokens (Audit H9).
 */
export function timingSafeTokenEqual(provided: string, expected: string): boolean {
  const a = Buffer.from(provided, 'utf8');
  const b = Buffer.from(expected, 'utf8');
  if (a.length !== b.length || a.length === 0) return false;
  return crypto.timingSafeEqual(a, b);
}

export function extractBearerToken(authHeader: string | undefined): string {
  if (!authHeader) return '';
  const match = /^Bearer\s+(.+)$/i.exec(authHeader.trim());
  return match ? match[1].trim() : '';
}

export interface AuthDecision {
  allowed: boolean;
  status: number;
  code?: string;
  error?: string;
}

export function isLoopbackAddress(addr: string | undefined): boolean {
  if (!addr) return false;
  const cleaned = addr.trim().toLowerCase().replace(/^::ffff:/, '');
  return cleaned === '127.0.0.1' || cleaned === '::1' || cleaned === 'localhost';
}

/**
 * Enforce Bearer authentication when:
 * - `SPECTRACRAFT_REMOTE_MODE=1`, OR
 * - `SPECTRACRAFT_AUTH_TOKEN` is configured, OR
 * - Bound to a non-loopback interface in production (`NODE_ENV=production`), OR
 * - Bound to a non-loopback interface in development and the client socket is non-loopback
 *   (Audit H9).
 * Loopback-only local usage and local dev proxies on `127.0.0.1` remain frictionless unless
 * a token or remote mode is enabled.
 */
export function checkRemoteAccessAuth(
  authHeader: string | undefined,
  bindHost: string,
  env: NodeJS.ProcessEnv = process.env,
  remoteAddress?: string,
): AuthDecision {
  const remoteMode = env.SPECTRACRAFT_REMOTE_MODE === '1' || env.SPECTRACRAFT_REMOTE_MODE === 'true';
  const nonLoopbackBind = !isLoopbackHost(bindHost);
  const isProd = env.NODE_ENV === 'production';
  const configuredToken = (env.SPECTRACRAFT_AUTH_TOKEN ?? '').trim();
  const isLocalClient = remoteAddress ? isLoopbackAddress(remoteAddress) : false;

  const requiresAuth =
    remoteMode ||
    Boolean(configuredToken) ||
    (nonLoopbackBind && (isProd || !isLocalClient));

  if (!requiresAuth) {
    return { allowed: true, status: 200 };
  }
  if (!configuredToken) {
    return {
      allowed: false,
      status: 503,
      code: 'AUTH_MISCONFIGURED',
      error: 'Remote or non-loopback server mode requires SPECTRACRAFT_AUTH_TOKEN to be configured.',
    };
  }
  const provided = extractBearerToken(authHeader);
  if (!provided || !timingSafeTokenEqual(provided, configuredToken)) {
    return {
      allowed: false,
      status: 401,
      code: 'UNAUTHORIZED',
      error: 'Valid Bearer authorization token is required.',
    };
  }
  return { allowed: true, status: 200 };
}

/**
 * Enforce optional admin token on privileged maintenance routes (`/api/audit/run`,
 * `/api/studio/reload-catalogs`, `/api/updates/action`, `/api/settings/reset`) and disable
 * source-tree update actions in production unless explicitly opted in (Audit H9).
 */
export function checkMaintenanceAuth(
  routePath: string,
  authHeader: string | undefined,
  env: NodeJS.ProcessEnv = process.env,
): AuthDecision {
  const isProd = env.NODE_ENV === 'production';
  const devUpdatesEnabled =
    env.SPECTRACRAFT_ENABLE_DEV_UPDATES === '1' || env.SPECTRACRAFT_ENABLE_DEV_UPDATES === 'true';
  if (routePath === '/api/updates/action' && isProd && !devUpdatesEnabled) {
    return {
      allowed: false,
      status: 403,
      code: 'UPDATES_DISABLED_IN_PRODUCTION',
      error:
        'In-place dependency update actions are disabled in production. Set SPECTRACRAFT_ENABLE_DEV_UPDATES=1 only in trusted development environments.',
    };
  }
  const adminToken = (env.SPECTRACRAFT_ADMIN_TOKEN ?? '').trim();
  if (!adminToken) {
    return { allowed: true, status: 200 };
  }
  const provided = extractBearerToken(authHeader);
  if (!provided || !timingSafeTokenEqual(provided, adminToken)) {
    return {
      allowed: false,
      status: 403,
      code: 'ADMIN_TOKEN_REQUIRED',
      error: 'Valid administrator Bearer token is required for this maintenance endpoint.',
    };
  }
  return { allowed: true, status: 200 };
}

/**
 * Full-origin check for mutating API requests (Audit H9):
 * - Validates full scheme + host (never bare hostname only).
 * - Ignores `X-Forwarded-Host` unless `SPECTRACRAFT_TRUST_PROXY=1`.
 * - Honors comma-separated `SPECTRACRAFT_ALLOWED_ORIGINS` allowlist.
 */
export function isAllowedMutationOrigin(
  originHeader: string | undefined,
  reqHost: string | undefined,
  forwardedHost: string | undefined,
  protocol: string,
  env: NodeJS.ProcessEnv = process.env,
): { allowed: boolean; reason?: string } {
  if (!originHeader || originHeader.trim().length === 0) {
    return { allowed: true };
  }
  let parsedOrigin: URL;
  try {
    parsedOrigin = new URL(originHeader.trim());
  } catch {
    return { allowed: false, reason: 'Malformed Origin header.' };
  }
  if (parsedOrigin.protocol !== 'http:' && parsedOrigin.protocol !== 'https:') {
    return { allowed: false, reason: 'Unsupported Origin scheme.' };
  }

  const normalizeOrigin = (u: URL) =>
    `${u.protocol}//${u.host.toLowerCase().replace(/^127\.0\.0\.1(?=:|$)/, 'localhost')}`;
  const originNormalized = normalizeOrigin(parsedOrigin);

  // Explicit full-origin allowlist
  const allowlistRaw = (env.SPECTRACRAFT_ALLOWED_ORIGINS ?? '').trim();
  if (allowlistRaw) {
    const allowedOrigins = new Set<string>();
    for (const part of allowlistRaw.split(',')) {
      const trimmed = part.trim();
      if (!trimmed) continue;
      try {
        allowedOrigins.add(normalizeOrigin(new URL(trimmed)));
      } catch {
        // ignore invalid entries
      }
    }
    if (allowedOrigins.has(originNormalized)) {
      return { allowed: true };
    }
  }

  const trustProxy =
    env.SPECTRACRAFT_TRUST_PROXY === '1' || env.SPECTRACRAFT_TRUST_PROXY === 'true';
  const candidateHosts: string[] = [];
  if (reqHost && reqHost.trim()) candidateHosts.push(reqHost.trim().toLowerCase());
  if (trustProxy && forwardedHost && forwardedHost.trim()) {
    const firstFwd = forwardedHost.split(',')[0].trim().toLowerCase();
    if (firstFwd) candidateHosts.push(firstFwd);
  }

  const scheme = protocol === 'https' || protocol === 'https:' ? 'https:' : 'http:';
  for (const h of candidateHosts) {
    const normHost = h.replace(/^127\.0\.0\.1(?=:|$)/, 'localhost');
    if (
      originNormalized === `${scheme}//${normHost}` ||
      (trustProxy && originNormalized === `https://${normHost}`)
    ) {
      return { allowed: true };
    }
  }

  return { allowed: false, reason: 'Cross-origin API mutation blocked.' };
}

/**
 * Decode Unicode upload filename from `X-Filename-Encoded` (URI-encoded UTF-8) with
 * safe ASCII fallback `X-Filename` (Audit M6). Display name is kept strictly separate
 * from opaque UUID storage paths on disk.
 */
export function decodeUploadDisplayName(
  headers: Record<string, string | string[] | undefined>,
): string {
  const firstHeader = (key: string): string => {
    const val = headers[key];
    if (Array.isArray(val)) return String(val[0] ?? '');
    return typeof val === 'string' ? val : '';
  };

  let decoded = '';
  const encoded = firstHeader('x-filename-encoded').trim();
  if (encoded) {
    try {
      decoded = decodeURIComponent(encoded);
    } catch {
      decoded = '';
    }
  }
  if (!decoded) {
    decoded = firstHeader('x-filename').trim();
  }
  const base = path.basename(decoded.replace(/\\/g, '/'));
  const clean = base
    .replace(/[\x00-\x1f\x7f<>:"/\\|?*]/g, '_')
    .trim()
    .slice(0, 120);
  return clean || 'uploaded-audio.wav';
}

export interface WorkspaceOwnershipLease {
  sessionsRoot: string;
  lockPath: string;
  isolated: boolean;
  canPruneBase: boolean;
  release: () => void;
}

export function defaultIsPidAlive(pid: number): boolean {
  if (!Number.isInteger(pid) || pid <= 0) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (err) {
    const code = (err as NodeJS.ErrnoException)?.code;
    return code === 'EPERM';
  }
}

/**
 * Acquire exclusive ownership of `baseRoot` via an atomic `.server-owner.lock` file (`wx` / `O_CREAT|O_EXCL`).
 * If another LIVE server process owns `baseRoot`, this instance MUST NOT prune `baseRoot` and instead
 * isolates itself in a dedicated per-PID subdirectory (Audit H10).
 */
export function acquireSessionsWorkspace(
  baseRoot: string,
  pid: number = process.pid,
  isPidAlive: (pid: number) => boolean = defaultIsPidAlive,
): WorkspaceOwnershipLease {
  fs.mkdirSync(baseRoot, { recursive: true, mode: 0o700 });
  const baseLock = path.join(baseRoot, '.server-owner.lock');
  const payload = JSON.stringify({ pid, startedAt: new Date().toISOString() });

  const tryCreateLock = (lockFile: string): boolean => {
    try {
      fs.writeFileSync(lockFile, payload, { flag: 'wx', mode: 0o600 });
      return true;
    } catch (err) {
      if ((err as NodeJS.ErrnoException)?.code === 'EEXIST') return false;
      throw err;
    }
  };

  const makeRelease = (lockFile: string) => () => {
    try {
      const raw = fs.readFileSync(lockFile, 'utf8');
      const parsed = JSON.parse(raw) as { pid?: number };
      if (parsed.pid === pid) {
        fs.unlinkSync(lockFile);
      }
    } catch {
      // ignore if already removed
    }
  };

  if (tryCreateLock(baseLock)) {
    return {
      sessionsRoot: baseRoot,
      lockPath: baseLock,
      isolated: false,
      canPruneBase: true,
      release: makeRelease(baseLock),
    };
  }

  // Lock exists — check whether owning PID is still alive
  let ownerAlive = true;
  try {
    const parsed = JSON.parse(fs.readFileSync(baseLock, 'utf8')) as { pid?: number };
    if (typeof parsed.pid === 'number' && parsed.pid !== pid && !isPidAlive(parsed.pid)) {
      ownerAlive = false;
    }
  } catch {
    // Unparseable lock file — treat defensively as live to avoid destroying another server's state
    ownerAlive = true;
  }

  if (!ownerAlive) {
    try {
      fs.unlinkSync(baseLock);
    } catch {
      // another process raced to remove it
    }
    if (tryCreateLock(baseLock)) {
      return {
        sessionsRoot: baseRoot,
        lockPath: baseLock,
        isolated: false,
        canPruneBase: true,
        release: makeRelease(baseLock),
      };
    }
  }

  // Another live server owns baseRoot -> isolate into dedicated per-PID workspace without touching baseRoot
  const isolatedRoot = path.join(baseRoot, `pid-${pid}-${crypto.randomUUID().slice(0, 6)}`);
  fs.mkdirSync(isolatedRoot, { recursive: true, mode: 0o700 });
  const isolatedLock = path.join(isolatedRoot, '.server-owner.lock');
  fs.writeFileSync(isolatedLock, payload, { flag: 'wx', mode: 0o600 });
  return {
    sessionsRoot: isolatedRoot,
    lockPath: isolatedLock,
    isolated: true,
    canPruneBase: false,
    release: makeRelease(isolatedLock),
  };
}

/**
 * Active artifact lease tracker preventing eviction from unlinking files currently being
 * streamed over `/api/media/:key` or consumed by an in-flight Rust job (Audit H11).
 */
export class ArtifactLeaseManager {
  private readonly leases = new Map<string, number>();

  public acquire(filePath: string): () => void {
    const abs = path.resolve(filePath);
    this.leases.set(abs, (this.leases.get(abs) ?? 0) + 1);
    let released = false;
    return () => {
      if (released) return;
      released = true;
      const count = (this.leases.get(abs) ?? 0) - 1;
      if (count <= 0) {
        this.leases.delete(abs);
      } else {
        this.leases.set(abs, count);
      }
    };
  }

  public isLeased(filePath: string): boolean {
    const abs = path.resolve(filePath);
    return (this.leases.get(abs) ?? 0) > 0;
  }

  public hasLeaseUnder(targetPath: string): boolean {
    const absTarget = path.resolve(targetPath);
    for (const [leasedPath, count] of this.leases.entries()) {
      if (count <= 0) continue;
      if (leasedPath === absTarget || leasedPath.startsWith(absTarget + path.sep)) {
        return true;
      }
    }
    return false;
  }
}

export type StrictArtifactResolution =
  | { ok: true; filePath: string; isBaseline: boolean }
  | { ok: false; status: 404 | 410; code: 'ARTIFACT_NOT_FOUND' | 'ARTIFACT_EXPIRED'; error: string };

/**
 * Strict run-ID artifact lookup: NEVER silently falls back to baseline when a non-empty run ID
 * was requested (Audit H5). Returns 410 if the run ID was evicted, or 404 if unknown/missing.
 */
export function resolveStrictRunArtifact(
  runId: string,
  runMap: ReadonlyMap<string, string>,
  evictedRunIds: ReadonlySet<string>,
  baselinePath: string,
  label: string,
): StrictArtifactResolution {
  const cleanId = runId.trim();
  if (!cleanId) {
    if (!fs.existsSync(baselinePath)) {
      return {
        ok: false,
        status: 404,
        code: 'ARTIFACT_NOT_FOUND',
        error: `Baseline ${label} not found for this session.`,
      };
    }
    return { ok: true, filePath: baselinePath, isBaseline: true };
  }

  const found = runMap.get(cleanId);
  if (found && fs.existsSync(found)) {
    return { ok: true, filePath: found, isBaseline: false };
  }
  if (evictedRunIds.has(cleanId)) {
    return {
      ok: false,
      status: 410,
      code: 'ARTIFACT_EXPIRED',
      error: `Requested ${label} revision '${cleanId}' has expired and been evicted. Re-run the stage before exporting.`,
    };
  }
  return {
    ok: false,
    status: 404,
    code: 'ARTIFACT_NOT_FOUND',
    error: `Requested ${label} revision '${cleanId}' was not found in this session.`,
  };
}

/**
 * Recursively measure real byte and file usage inside a directory (Audit M8).
 */
export function measureDirectoryUsage(dirPath: string): { bytes: number; files: number } {
  let bytes = 0;
  let files = 0;
  if (!fs.existsSync(dirPath)) return { bytes: 0, files: 0 };
  const stack = [dirPath];
  while (stack.length > 0) {
    const current = stack.pop()!;
    let entries: fs.Dirent[];
    try {
      entries = fs.readdirSync(current, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      if (entry.isSymbolicLink()) continue;
      const full = path.join(current, entry.name);
      if (entry.isDirectory()) {
        stack.push(full);
      } else if (entry.isFile()) {
        try {
          const st = fs.statSync(full);
          bytes += st.size;
          files += 1;
        } catch {
          // ignore concurrent unlink
        }
      }
    }
  }
  return { bytes, files };
}

/**
 * Convert Express body-parser or unhandled route errors into structured JSON (Audit M14),
 * ensuring malformed JSON (400), oversized payloads (413), and unexpected errors (500)
 * never emit HTML error pages or stack traces.
 */
export function mapExpressErrorToJson(
  err: unknown,
  sensitiveRoots: readonly string[] = [],
): { status: number; body: { error: string; code: string } } {
  const e = err as { type?: string; status?: number; statusCode?: number; message?: string } | null;
  const isSyntax = err instanceof SyntaxError;
  if (e?.type === 'entity.parse.failed' || (isSyntax && e?.status === 400)) {
    return {
      status: 400,
      body: { error: 'Malformed JSON request body.', code: 'INVALID_JSON' },
    };
  }
  if (e?.type === 'entity.too.large' || e?.status === 413 || e?.statusCode === 413) {
    return {
      status: 413,
      body: { error: 'Request payload exceeds the allowed size limit.', code: 'PAYLOAD_TOO_LARGE' },
    };
  }
  const status =
    typeof e?.status === 'number' && e.status >= 400 && e.status < 600
      ? e.status
      : typeof e?.statusCode === 'number' && e.statusCode >= 400 && e.statusCode < 600
        ? e.statusCode
        : 500;
  const safeMsg = sanitizeClientErrorMessage(
    e?.message ? String(e.message) : '',
    sensitiveRoots,
    status >= 500 ? 'Internal server error.' : 'Request failed.',
  );
  return {
    status,
    body: {
      error: safeMsg,
      code: status >= 500 ? 'INTERNAL_ERROR' : 'BAD_REQUEST',
    },
  };
}
