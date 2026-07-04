import assert from 'node:assert/strict';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

import { validateReleaseAssets } from '../../scripts/check-release-assets.mjs';

const validNativeRuntimeProfile = JSON.parse(
    readFileSync(new URL('../../src-tauri/native-llama-runtime.json', import.meta.url), 'utf8'),
);

const validCargoToml = `
[features]
native-llama = ["dep:llama-cpp-2", "llama-cpp-2/common", "llama-cpp-2/sampler"]
native-llama-metal = ["native-llama", "llama-cpp-2/metal"]

[dependencies]
llama-cpp-2 = { version = "0.1.150", default-features = false, optional = true }
`;

function createReleaseProject(t, overrides = {}) {
    const root = mkdtempSync(join(tmpdir(), 'horary-release-assets-'));
    t.after(() => rmSync(root, { recursive: true, force: true }));

    mkdirSync(join(root, 'src-tauri'), { recursive: true });
    writeJson(join(root, 'src-tauri', 'tauri.conf.json'), overrides.tauriConfig ?? {});
    writeJson(
        join(root, 'src-tauri', 'tauri.release.conf.json'),
        overrides.tauriReleaseConfig ?? { bundle: { externalBin: [] } },
    );
    writeJson(
        join(root, 'src-tauri', 'native-llama-runtime.json'),
        overrides.nativeRuntime ?? validNativeRuntimeProfile,
    );
    writeJson(
        join(root, 'src-tauri', 'model-manifest.json'),
        overrides.manifest ?? {
            version: '2026-07-01',
            updatedAt: '2026-07-01',
            models: [],
        },
    );
    writeFileSync(join(root, 'src-tauri', 'Cargo.toml'), overrides.cargoToml ?? validCargoToml);

    return root;
}

function writeJson(path, value) {
    writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
}

function assertIssue(result, expected) {
    assert.equal(
        result.issues.some(issue => issue.includes(expected)),
        true,
        `expected issue containing "${expected}", got:\n${result.issues.join('\n')}`,
    );
}

test('release asset gate accepts native runtime without bundled fallback sidecars or catalog models', t => {
    const root = createReleaseProject(t);

    const result = validateReleaseAssets({ root });

    assert.equal(result.ok, true);
    assert.equal(result.nativeRuntimeReady, true);
    assert.equal(result.sidecarRequired, false);
    assert.equal(result.enabledModelCount, 0);
});

test('release asset gate makes sidecar artifacts mandatory when the native runtime is malformed', t => {
    const nativeRuntime = structuredClone(validNativeRuntimeProfile);
    nativeRuntime.rustBinding.version = '0.0.0';
    nativeRuntime.tauriBoundary.frontendTransport = 'http-loopback';

    const root = createReleaseProject(t, { nativeRuntime });
    const result = validateReleaseAssets({ root });

    assert.equal(result.ok, false);
    assert.equal(result.nativeRuntimeReady, false);
    assert.equal(result.sidecarRequired, true);
    assertIssue(result, 'llama-cpp-2 version must be 0.1.150');
    assertIssue(result, 'must keep frontend inference transport on Tauri IPC');
    assertIssue(result, 'bundle.externalBin');
    assertIssue(result, 'Missing release sidecar binary');
});

test('release asset gate verifies optional sidecar checksums when sidecars are present', t => {
    const root = createReleaseProject(t);
    const binariesDir = join(root, 'src-tauri', 'binaries');
    const binaryPath = join(binariesDir, 'llama-server-aarch64-apple-darwin');

    mkdirSync(binariesDir, { recursive: true });
    writeFileSync(binaryPath, 'fake sidecar');
    chmodSync(binaryPath, 0o755);
    writeFileSync(`${binaryPath}.sha256`, `${'0'.repeat(64)}  llama-server-aarch64-apple-darwin\n`);

    const result = validateReleaseAssets({ root });

    assert.equal(result.ok, false);
    assertIssue(result, 'Sidecar checksum mismatch');
});

test('release asset gate requires complete metadata for enabled catalog models', t => {
    const nativeRuntime = structuredClone(validNativeRuntimeProfile);
    nativeRuntime.modelPolicy.requireRemoteCatalogModel = true;
    const manifest = {
        version: '2026-07-01',
        updatedAt: '2026-07-01',
        models: [
            {
                id: 'gemma-test',
                displayName: 'Gemma Test',
                sha256: 'not-a-sha',
                source: { url: 'http://example.invalid/model.gguf' },
                sizeBytes: 0,
                minRamGb: 4,
                recommendedRamGb: 2,
                defaultContextTokens: 0,
                defaultParams: { temperature: 3, top_p: 0, max_tokens: 20000 },
            },
        ],
    };

    const root = createReleaseProject(t, { nativeRuntime, manifest });
    const result = validateReleaseAssets({ root });

    assert.equal(result.ok, false);
    assert.equal(result.enabledModelCount, 1);
    assertIssue(result, '64-character SHA-256 checksum');
    assertIssue(result, 'source.url with an HTTPS download URL');
    assertIssue(result, 'source.type');
    assertIssue(result, 'verified license');
    assertIssue(result, 'positive sizeBytes');
    assertIssue(result, 'recommendedRamGb greater than or equal to minRamGb');
    assertIssue(result, 'positive defaultContextTokens');
    assertIssue(result, 'defaultParams.temperature');
    assertIssue(result, 'defaultParams.top_p');
    assertIssue(result, 'defaultParams.max_tokens');
});
