import { resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { discoverGemmaGguf, discoverGemmaMtpGguf } from './native-llama-models.mjs';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const modelPath = process.env.HORARY_NATIVE_LLAMA_TEST_MODEL || discoverGemmaGguf();
const mtpModelPath = process.env.HORARY_NATIVE_LLAMA_MTP_MODEL || discoverGemmaMtpGguf();
if (!modelPath) {
    console.error('Native llama gate failed: no cached Gemma GGUF found. Set HORARY_NATIVE_LLAMA_TEST_MODEL to a local .gguf path.');
    process.exit(1);
}
if (process.env.HORARY_NATIVE_LLAMA_REQUIRE_MTP === '1' && !mtpModelPath) {
    console.error('Native llama MTP gate failed: no cached Gemma MTP GGUF found. Set HORARY_NATIVE_LLAMA_MTP_MODEL to a local .gguf path.');
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
console.log(`Native llama MTP model: ${mtpModelPath || 'not cached'}`);
console.log(`Native llama gate features: ${features}`);
const result = spawnSync('cargo', args, {
    cwd: root,
    stdio: 'inherit',
    env: {
        ...process.env,
        HORARY_NATIVE_LLAMA_TEST_MODEL: modelPath,
        ...(mtpModelPath ? { HORARY_NATIVE_LLAMA_MTP_MODEL: mtpModelPath } : {}),
    },
});

process.exit(result.status ?? 1);
