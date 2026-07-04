import { mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { discoverGemmaGguf, discoverGemmaMtpGguf } from './native-llama-models.mjs';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const modelPath = process.env.HORARY_NATIVE_LLAMA_TEST_MODEL || discoverGemmaGguf();
const mtpModelPath = process.env.HORARY_NATIVE_LLAMA_MTP_MODEL || discoverGemmaMtpGguf();
const reportPath = process.env.HORARY_READING_EVAL_REPORT
    || resolve(root, 'artifacts', 'horary-reading-eval.json');

if (!modelPath) {
    console.error('Horary reading eval failed: no cached Gemma GGUF found. Set HORARY_NATIVE_LLAMA_TEST_MODEL to a local .gguf path.');
    process.exit(1);
}
if (process.env.HORARY_NATIVE_LLAMA_REQUIRE_MTP === '1' && !mtpModelPath) {
    console.error('Horary reading eval failed: no cached Gemma MTP GGUF found. Set HORARY_NATIVE_LLAMA_MTP_MODEL to a local .gguf path.');
    process.exit(1);
}

mkdirSync(dirname(reportPath), { recursive: true });

const features = process.env.HORARY_NATIVE_LLAMA_FEATURES
    || (process.platform === 'darwin' ? 'native-llama-metal' : 'native-llama');
const args = [
    'test',
    '--manifest-path',
    'src-tauri/Cargo.toml',
    '--features',
    features,
    'ai::horary_reading_eval_tests',
    '--',
    '--nocapture',
];

console.log(`Horary reading eval model: ${modelPath}`);
console.log(`Horary reading eval MTP model: ${mtpModelPath || 'not cached'}`);
console.log(`Horary reading eval report: ${reportPath}`);
const result = spawnSync('cargo', args, {
    cwd: root,
    stdio: 'inherit',
    env: {
        ...process.env,
        HORARY_NATIVE_LLAMA_TEST_MODEL: modelPath,
        HORARY_READING_EVAL_REPORT: reportPath,
        ...(mtpModelPath ? { HORARY_NATIVE_LLAMA_MTP_MODEL: mtpModelPath } : {}),
    },
});

process.exit(result.status ?? 1);
