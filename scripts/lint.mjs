import { execFileSync } from 'node:child_process';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

const root = process.cwd();
const tauriConfigPath = join(root, 'src-tauri', 'tauri.conf.json');
const tauriCapabilitiesPath = join(root, 'src-tauri', 'capabilities', 'default.json');
const ignoredDirs = new Set(['node_modules', 'dist', 'target', '.git']);
const jsFiles = [];
const textFiles = [];
const browserRuntimeFiles = [];
const requiredTauriCspDirectives = [
    "default-src 'self'",
    "script-src 'self'",
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data:",
    "connect-src ipc: http://ipc.localhost",
    "object-src 'none'",
    "base-uri 'none'",
    "frame-ancestors 'none'",
];

function walk(dir) {
    for (const entry of readdirSync(dir)) {
        if (ignoredDirs.has(entry)) continue;
        const path = join(dir, entry);
        const stat = statSync(path);
        if (stat.isDirectory()) {
            walk(path);
            continue;
        }

        if (entry.endsWith('.js') || entry.endsWith('.mjs')) jsFiles.push(path);
        if (entry.endsWith('.css') || entry.endsWith('.html') || entry.endsWith('.js') || entry.endsWith('.mjs')) {
            textFiles.push(path);
        }
        if (isBrowserRuntimeFile(path)) browserRuntimeFiles.push(path);
    }
}

walk(root);

for (const file of jsFiles) {
    execFileSync(process.execPath, ['--check', file], { stdio: 'inherit' });
}

const forbiddenPatterns = [
    /fonts\.googleapis\.com/i,
    /fonts\.gstatic\.com/i,
];

const forbiddenBrowserRuntimePatterns = [
    {
        pattern: /\bfetch\s*\(/,
        message: 'browser runtime must not call fetch directly; keep remote/network work behind explicit Tauri or tested provider boundaries',
    },
    {
        pattern: /\bXMLHttpRequest\b/,
        message: 'browser runtime must not use XMLHttpRequest directly',
    },
    {
        pattern: /\bWebSocket\s*\(/,
        message: 'browser runtime must not open WebSocket connections directly',
    },
    {
        pattern: /\bEventSource\s*\(/,
        message: 'browser runtime must not open EventSource connections directly',
    },
    {
        pattern: /\bnavigator\.sendBeacon\b/,
        message: 'browser runtime must not send beacon telemetry',
    },
    {
        pattern: /\b(?:nominatim|timeapi\.io|overpass-api|maps\.googleapis)\b/i,
        message: 'browser runtime must not reference external geocoding/timezone providers',
    },
    {
        pattern: /https?:\/\/(?!www\.w3\.org\/2000\/svg\b)/i,
        message: 'browser runtime must not embed remote HTTP(S) URLs',
    },
];

let failed = false;
for (const file of textFiles) {
    const text = readFileSync(file, 'utf8');
    for (const pattern of forbiddenPatterns) {
        if (pattern.test(text)) {
            console.error(`${relative(root, file)} contains forbidden external font reference: ${pattern}`);
            failed = true;
        }
    }
}

for (const file of browserRuntimeFiles) {
    const text = readFileSync(file, 'utf8');
    for (const { pattern, message } of forbiddenBrowserRuntimePatterns) {
        if (pattern.test(text)) {
            console.error(`${relative(root, file)} violates browser network policy: ${message}`);
            failed = true;
        }
    }
}

checkTauriSecurityConfig();
checkTauriCapabilities();

if (failed) process.exit(1);

function checkTauriSecurityConfig() {
    const config = readJsonFile(tauriConfigPath);
    if (!config) return;

    const csp = config.app?.security?.csp;
    if (typeof csp !== 'string' || csp.trim() === '') {
        console.error('src-tauri/tauri.conf.json must define a production app.security.csp string.');
        failed = true;
        return;
    }

    for (const directive of requiredTauriCspDirectives) {
        if (!csp.includes(directive)) {
            console.error(`src-tauri/tauri.conf.json app.security.csp is missing directive: ${directive}`);
            failed = true;
        }
    }

    if (config.app?.security?.dangerousDisableAssetCspModification === true) {
        console.error('src-tauri/tauri.conf.json must not disable Tauri asset CSP modification.');
        failed = true;
    }
}

function checkTauriCapabilities() {
    const capabilities = readJsonFile(tauriCapabilitiesPath);
    if (!capabilities) return;

    const permissions = capabilities.permissions || [];
    const allowedPermissions = ['core:default'];
    const extraPermissions = permissions.filter(permission => !allowedPermissions.includes(permission));
    const missingPermissions = allowedPermissions.filter(permission => !permissions.includes(permission));

    if (extraPermissions.length > 0 || missingPermissions.length > 0) {
        console.error(`src-tauri/capabilities/default.json must grant only ${allowedPermissions.join(', ')}.`);
        failed = true;
    }
}

function readJsonFile(path) {
    try {
        return JSON.parse(readFileSync(path, 'utf8'));
    } catch (error) {
        console.error(`${relative(root, path)} is not valid JSON: ${error.message}`);
        failed = true;
        return null;
    }
}

function isBrowserRuntimeFile(path) {
    const rel = relative(root, path);
    if (rel === 'index.html' || rel === 'vite.config.js') return true;
    if (!rel.startsWith(`src${sep()}`)) return false;
    if (rel.includes(`${sep()}__tests__${sep()}`) || rel.includes(`${sep()}__fixtures__${sep()}`)) {
        return false;
    }
    return /\.(?:js|mjs|html|css)$/.test(rel);
}

function sep() {
    return process.platform === 'win32' ? '\\' : '/';
}
