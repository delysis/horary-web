import { existsSync, readdirSync, statSync } from 'node:fs';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const modelPath = process.env.WHORARY_NATIVE_LLAMA_TEST_MODEL || discoverGemmaGguf();
const mtpModelPath = process.env.WHORARY_NATIVE_LLAMA_MTP_MODEL || discoverGemmaMtpGguf();
if (!modelPath) {
    console.error('Native llama gate failed: no cached Gemma GGUF found. Set WHORARY_NATIVE_LLAMA_TEST_MODEL to a local .gguf path.');
    process.exit(1);
}
if (process.env.WHORARY_NATIVE_LLAMA_REQUIRE_MTP === '1' && !mtpModelPath) {
    console.error('Native llama MTP gate failed: no cached Gemma MTP GGUF found. Set WHORARY_NATIVE_LLAMA_MTP_MODEL to a local .gguf path.');
    process.exit(1);
}

const features = process.env.WHORARY_NATIVE_LLAMA_FEATURES
    || (process.platform === 'darwin' ? 'native-llama-metal' : 'native-llama');
const args = [
    'test',
    '--manifest-path',
    'src-tauri/Cargo.toml',
    '--features',
    features,
    'native_llama_worker::integration_tests',
    '--',
    '--nocapture',
];

console.log(`Native llama gate model: ${modelPath}`);
console.log(`Native llama MTP model: ${mtpModelPath || 'not cached'}`);
console.log(`Native llama gate features: ${features}`);
const result = spawnSync('cargo', args, {
    cwd: root,
    stdio: 'inherit',
    env: {
        ...process.env,
        WHORARY_NATIVE_LLAMA_TEST_MODEL: modelPath,
        ...(mtpModelPath ? { WHORARY_NATIVE_LLAMA_MTP_MODEL: mtpModelPath } : {}),
    },
});

process.exit(result.status ?? 1);

function discoverGemmaGguf() {
    const hub = join(homedir(), '.cache', 'huggingface', 'hub');
    const candidates = [
        'models--unsloth--gemma-4-E2B-it-GGUF',
        'models--ggml-org--gemma-4-E2B-it-GGUF',
        'models--ggml-org--gemma-4-E4B-it-GGUF',
    ]
        .flatMap(repo => ggufFiles(join(hub, repo, 'snapshots')))
        .filter(path => !path.includes('mmproj'))
        .sort((a, b) => modelRank(a) - modelRank(b));
    return candidates[0] || null;
}

function discoverGemmaMtpGguf() {
    const hub = join(homedir(), '.cache', 'huggingface', 'hub');
    const candidates = [
        'models--unsloth--gemma-4-E2B-it-GGUF',
        'models--ggml-org--gemma-4-E2B-it-GGUF',
        'models--ggml-org--gemma-4-E4B-it-GGUF',
    ]
        .flatMap(repo => ggufFiles(join(hub, repo, 'snapshots')))
        .filter(path => path.toLowerCase().includes('mtp'))
        .sort((a, b) => mtpModelRank(a) - mtpModelRank(b));
    return candidates[0] || null;
}

function ggufFiles(rootDir) {
    if (!existsSync(rootDir)) return [];
    const result = [];
    const stack = [rootDir];
    while (stack.length > 0) {
        const dir = stack.pop();
        for (const entry of readdirSync(dir, { withFileTypes: true })) {
            const path = join(dir, entry.name);
            if (entry.isDirectory()) {
                stack.push(path);
            } else if (entry.isFile() || entry.isSymbolicLink()) {
                if (entry.name.toLowerCase().endsWith('.gguf') && statSync(path).size > 0) {
                    result.push(path);
                }
            }
        }
    }
    return result;
}

function modelRank(path) {
    const lower = path.toLowerCase();
    if (lower.includes('e2b') && lower.includes('q6_k')) return 0;
    if (lower.includes('e2b') && lower.includes('q8_0')) return 1;
    if (lower.includes('e4b')) return 2;
    return 10;
}

function mtpModelRank(path) {
    const lower = path.toLowerCase();
    if (lower.includes('e2b') && lower.includes('q8_0')) return 0;
    if (lower.includes('e2b') && lower.includes('f16')) return 1;
    if (lower.includes('e2b') && lower.includes('bf16')) return 2;
    if (lower.includes('e4b')) return 3;
    return 10;
}
