import { existsSync, readdirSync, statSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

export function discoverGemmaGguf() {
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

export function discoverGemmaMtpGguf() {
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
