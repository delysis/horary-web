import { resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { discoverGemmaGguf } from './native-llama-models.mjs';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const modelPath = process.env.HORARY_NATIVE_LLAMA_TEST_MODEL || discoverGemmaGguf();
if (!modelPath) {
    console.error('Native llama gate failed: no cached Gemma GGUF found. Set HORARY_NATIVE_LLAMA_TEST_MODEL to a local .gguf path.');
    process.exit(1);
}
if (process.env.HORARY_NATIVE_LLAMA_REQUIRE_MTP === '1') {
    console.error('The pinned native-kit runtime does not expose speculative decoding.');
    process.exit(1);
}

const features = process.env.HORARY_NATIVE_LLAMA_FEATURES
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
console.log(`Native llama gate features: ${features}`);
const result = spawnSync('cargo', args, {
    cwd: root,
    stdio: 'inherit',
    env: {
        ...process.env,
        HORARY_NATIVE_LLAMA_TEST_MODEL: modelPath,
    },
});

process.exit(result.status ?? 1);
