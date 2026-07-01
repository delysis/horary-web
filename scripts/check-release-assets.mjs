import { createHash } from 'node:crypto';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export const expectedSidecars = [
    { name: 'llama-server-aarch64-apple-darwin', executable: true },
    { name: 'llama-server-x86_64-apple-darwin', executable: true },
    { name: 'llama-server-x86_64-pc-windows-msvc.exe', executable: false },
    { name: 'llama-server-x86_64-unknown-linux-gnu', executable: true },
];

export function validateReleaseAssets({ root = process.cwd() } = {}) {
    const projectRoot = resolve(root);
    const issues = [];
    const paths = {
        tauriConfig: join(projectRoot, 'src-tauri', 'tauri.conf.json'),
        tauriReleaseConfig: join(projectRoot, 'src-tauri', 'tauri.release.conf.json'),
        manifest: join(projectRoot, 'src-tauri', 'model-manifest.json'),
        nativeRuntime: join(projectRoot, 'src-tauri', 'native-llama-runtime.json'),
        cargoToml: join(projectRoot, 'src-tauri', 'Cargo.toml'),
    };

    readJson(paths.tauriConfig, issues);
    const tauriReleaseConfig = readJson(paths.tauriReleaseConfig, issues);
    const nativeRuntime = readJson(paths.nativeRuntime, issues);
    const nativeRuntimeIssues = validateNativeRuntime(nativeRuntime, paths, issues);
    const nativeRuntimeReady = nativeRuntimeIssues.length === 0;
    issues.push(...nativeRuntimeIssues);

    const externalBin = tauriReleaseConfig.bundle?.externalBin || [];
    const fallbackReleaseBlocking = nativeRuntime?.fallback?.releaseBlocking === true;
    const sidecarRequired = !nativeRuntimeReady || fallbackReleaseBlocking;

    if (sidecarRequired && !externalBin.includes('binaries/llama-server')) {
        issues.push('src-tauri/tauri.release.conf.json must include bundle.externalBin: ["binaries/llama-server"] when sidecar fallback is release-blocking.');
    }

    for (const sidecar of expectedSidecars) {
        validateSidecarAsset(projectRoot, sidecar, { required: sidecarRequired }, issues);
    }

    const manifest = readJson(paths.manifest, issues);
    const models = Array.isArray(manifest.models) ? manifest.models : [];
    const enabledModels = models.filter(model => model.enabled !== false);
    const remoteCatalogRequired = nativeRuntime?.modelPolicy?.requireRemoteCatalogModel !== false;
    if (remoteCatalogRequired && enabledModels.length === 0) {
        issues.push('src-tauri/model-manifest.json must contain at least one enabled release-verified model entry when remote catalog models are required.');
    }

    for (const model of enabledModels) {
        validateModelEntry(model, issues);
    }

    return {
        ok: issues.length === 0,
        issues,
        nativeRuntimeReady,
        sidecarRequired,
        enabledModelCount: enabledModels.length,
    };
}

export function releaseAssetGatePassMessage(result) {
    return result.nativeRuntimeReady
        ? 'Release asset gate passed for native llama.cpp runtime profile.'
        : 'Release asset gate passed.';
}

function validateSidecarAsset(projectRoot, sidecar, { required }, issues) {
    const binaryName = sidecar.name;
    const binaryPath = join(projectRoot, 'src-tauri', 'binaries', binaryName);
    const checksumPath = `${binaryPath}.sha256`;
    const hasBinary = existsSync(binaryPath);
    const hasChecksum = existsSync(checksumPath);

    if (!hasBinary && !required) {
        return;
    }
    const expectedSha256 = readSha256File(checksumPath, `src-tauri/binaries/${binaryName}.sha256`, { required: required || hasBinary || hasChecksum }, issues);
    if (!hasBinary) {
        issues.push(`Missing release sidecar binary: src-tauri/binaries/${binaryName}`);
        return;
    }

    const stat = statSync(binaryPath);
    if (!stat.isFile() || stat.size === 0) {
        issues.push(`Sidecar binary is empty or not a file: src-tauri/binaries/${binaryName}`);
        return;
    }
    if (sidecar.executable && (stat.mode & 0o111) === 0) {
        issues.push(`Sidecar binary must be executable: src-tauri/binaries/${binaryName}`);
    }
    if (expectedSha256) {
        const actualSha256 = sha256File(binaryPath);
        if (actualSha256 !== expectedSha256) {
            issues.push(`Sidecar checksum mismatch for src-tauri/binaries/${binaryName}: expected ${expectedSha256}, got ${actualSha256}`);
        }
    }
}

function validateModelEntry(model, issues) {
    if (!model.id || !model.displayName) {
        issues.push('Every model manifest entry must include id and displayName.');
    }
    if ((model.format || 'gguf') !== 'gguf') {
        issues.push(`Model ${model.id || '<unknown>'} must use GGUF format.`);
    }
    if (!model.sha256 || !/^[a-f0-9]{64}$/i.test(model.sha256)) {
        issues.push(`Model ${model.id || '<unknown>'} must include a 64-character SHA-256 checksum.`);
    }
    const url = model.source?.url;
    if (!url || !/^https:\/\//i.test(url)) {
        issues.push(`Model ${model.id || '<unknown>'} must include source.url with an HTTPS download URL.`);
    }
    if (!model.source?.type) {
        issues.push(`Model ${model.id || '<unknown>'} must include source.type.`);
    }
    if (!model.license) {
        issues.push(`Model ${model.id || '<unknown>'} must include a verified license.`);
    }
    if (!Number.isFinite(model.sizeBytes) || model.sizeBytes <= 0) {
        issues.push(`Model ${model.id || '<unknown>'} must include a positive sizeBytes value.`);
    }
    if (!Number.isFinite(model.minRamGb) || model.minRamGb <= 0) {
        issues.push(`Model ${model.id || '<unknown>'} must include a positive minRamGb value.`);
    }
    if (!Number.isFinite(model.recommendedRamGb) || model.recommendedRamGb < model.minRamGb) {
        issues.push(`Model ${model.id || '<unknown>'} must include recommendedRamGb greater than or equal to minRamGb.`);
    }
    if (!Number.isFinite(model.defaultContextTokens) || model.defaultContextTokens <= 0) {
        issues.push(`Model ${model.id || '<unknown>'} must include a positive defaultContextTokens value.`);
    }
    validateDefaultParams(model, issues);
}

function validateNativeRuntime(profile, paths, issues) {
    const runtimeIssues = [];
    if (!profile || typeof profile !== 'object' || Array.isArray(profile)) {
        return ['src-tauri/native-llama-runtime.json must define the native llama.cpp runtime profile.'];
    }

    if (profile.preferred !== true) {
        runtimeIssues.push('native runtime profile must set preferred: true.');
    }
    if (profile.backendId !== 'native-llama-cpp') {
        runtimeIssues.push('native runtime profile backendId must be native-llama-cpp.');
    }
    if (profile.rustBinding?.crate !== 'llama-cpp-2') {
        runtimeIssues.push('native runtime profile must use llama-cpp-2 Rust bindings.');
    }
    if (profile.rustBinding?.version !== '0.1.150') {
        runtimeIssues.push('native runtime profile llama-cpp-2 version must be 0.1.150.');
    }
    if (profile.rustBinding?.cargoFeature !== 'native-llama') {
        runtimeIssues.push('native runtime profile must name the native-llama Cargo feature.');
    }
    if (profile.tauriBoundary?.frontendTransport !== 'tauri-ipc' || profile.tauriBoundary?.frontendLoopbackHttp !== false) {
        runtimeIssues.push('native runtime profile must keep frontend inference transport on Tauri IPC with no browser-visible loopback HTTP.');
    }
    if (profile.modelPolicy?.allowUserImportedGguf !== true) {
        runtimeIssues.push('native runtime profile must allow user-imported GGUF models.');
    }
    if (profile.capabilities?.inProcessWorker !== true) {
        runtimeIssues.push('native runtime profile must require an in-process worker.');
    }
    if (profile.capabilities?.continuousBatching?.enabled !== true) {
        runtimeIssues.push('native runtime profile must require continuous batching.');
    }
    if (!Number.isInteger(profile.capabilities?.continuousBatching?.defaultParallelSequences) || profile.capabilities.continuousBatching.defaultParallelSequences < 2) {
        runtimeIssues.push('native runtime profile must default to at least two parallel sequences.');
    }
    if (!Number.isInteger(profile.capabilities?.continuousBatching?.maxParallelSequences) || profile.capabilities.continuousBatching.maxParallelSequences < profile.capabilities.continuousBatching.defaultParallelSequences) {
        runtimeIssues.push('native runtime profile must set maxParallelSequences greater than or equal to the default.');
    }
    const tiers = profile.capabilities?.kvCache?.tiers;
    if (!Array.isArray(tiers) || !tiers.some(tier => tier.id === 'hot') || !tiers.some(tier => tier.id === 'cold')) {
        runtimeIssues.push('native runtime profile must define hot and cold KV cache tiers.');
    }
    if (profile.capabilities?.speculativeDecoding?.preferredType !== 'draft-mtp') {
        runtimeIssues.push('native runtime profile must prefer draft-mtp speculative decoding.');
    }
    if (profile.capabilities?.speculativeDecoding?.enabledWhenDraftModelPresent !== true) {
        runtimeIssues.push('native runtime profile must enable draft-mtp when a draft model is present.');
    }
    if (profile.capabilities?.speculativeDecoding?.nativeWorkerActive !== true) {
        runtimeIssues.push('native runtime profile must expose draft-mtp nativeWorkerActive: true for the native worker.');
    }
    if (typeof profile.capabilities?.speculativeDecoding?.implementationStatus !== 'string') {
        runtimeIssues.push('native runtime profile must include speculativeDecoding.implementationStatus.');
    }
    if (profile.capabilities?.jsonConstrainedOutput?.required !== true) {
        runtimeIssues.push('native runtime profile must require JSON-constrained output.');
    }

    const cargoToml = readText(paths.cargoToml, issues);
    if (!cargoToml.includes('llama-cpp-2 = { version = "0.1.150"')) {
        runtimeIssues.push('src-tauri/Cargo.toml must declare optional llama-cpp-2 0.1.150 dependency.');
    }
    if (!cargoToml.includes('native-llama = ["dep:llama-cpp-2"')) {
        runtimeIssues.push('src-tauri/Cargo.toml must declare a native-llama feature using llama-cpp-2.');
    }
    if (!cargoToml.includes('native-llama-metal')) {
        runtimeIssues.push('src-tauri/Cargo.toml must expose a native-llama-metal feature for Apple Silicon release builds.');
    }

    return runtimeIssues;
}

function readJson(path, issues) {
    try {
        return JSON.parse(readFileSync(path, 'utf8'));
    } catch (error) {
        issues.push(`Unable to read JSON file ${path}: ${error.message}`);
        return {};
    }
}

function readText(path, issues) {
    try {
        return readFileSync(path, 'utf8');
    } catch (error) {
        issues.push(`Unable to read file ${path}: ${error.message}`);
        return '';
    }
}

function readSha256File(path, label, { required = true } = {}, issues) {
    if (!existsSync(path)) {
        if (required) {
            issues.push(`Missing sidecar checksum file: ${label}`);
        }
        return null;
    }

    const firstToken = readFileSync(path, 'utf8').trim().split(/\s+/)[0]?.toLowerCase();
    if (!/^[a-f0-9]{64}$/.test(firstToken || '')) {
        issues.push(`Sidecar checksum file must start with a 64-character SHA-256 hex digest: ${label}`);
        return null;
    }
    return firstToken;
}

function sha256File(path) {
    return createHash('sha256').update(readFileSync(path)).digest('hex');
}

function validateDefaultParams(model, issues) {
    const id = model.id || '<unknown>';
    const params = model.defaultParams;
    if (!params || typeof params !== 'object' || Array.isArray(params)) {
        issues.push(`Model ${id} must include defaultParams with temperature, top_p, and max_tokens.`);
        return;
    }

    if (!Number.isFinite(params.temperature) || params.temperature < 0 || params.temperature > 2) {
        issues.push(`Model ${id} defaultParams.temperature must be between 0 and 2.`);
    }
    if (!Number.isFinite(params.top_p) || params.top_p <= 0 || params.top_p > 1) {
        issues.push(`Model ${id} defaultParams.top_p must be greater than 0 and no more than 1.`);
    }
    if (!Number.isInteger(params.max_tokens) || params.max_tokens <= 0 || params.max_tokens > 16384) {
        issues.push(`Model ${id} defaultParams.max_tokens must be an integer between 1 and 16384.`);
    }
}

function isMainModule() {
    return Boolean(process.argv[1]) && import.meta.url === pathToFileURL(resolve(process.argv[1])).href;
}

if (isMainModule()) {
    const result = validateReleaseAssets();
    if (!result.ok) {
        console.error('Release asset gate failed:');
        for (const issue of result.issues) {
            console.error(`- ${issue}`);
        }
        process.exit(1);
    }

    console.log(releaseAssetGatePassMessage(result));
}
