import { existsSync, readFileSync, realpathSync, statSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

const manifest = JSON.parse(readFileSync(new URL('../src-tauri/model-manifest.json', import.meta.url), 'utf8'));
function hubRoot() {
    return process.env.HF_HUB_CACHE || process.env.HUGGINGFACE_HUB_CACHE
        || (process.env.HF_HOME ? join(process.env.HF_HOME, 'hub')
            : join(process.env.XDG_CACHE_HOME || join(homedir(), '.cache'), 'huggingface', 'hub'));
}
function pinnedFile(id) {
    const entry = manifest.models.find(model => model.id === id);
    if (!entry) return null;
    const repo = join(hubRoot(), `models--${entry.source.repo.replaceAll('/', '--')}`);
    for (const path of [join(repo, 'snapshots', entry.source.revision, entry.source.filename), join(repo, 'blobs', entry.sha256)]) {
        if (existsSync(path) && statSync(path).size === entry.sizeBytes) return path;
    }
    return null;
}
export function discoverGemmaGguf() { return pinnedFile('gemma-4-12b-qat'); }
export function discoverGemmaMtpGguf(modelPath) {
    const target = discoverGemmaGguf();
    // Never silently pair a user's different model with the recommended drafter.
    if (!modelPath || !target || realpathSync(target) !== realpathSync(modelPath)) return null;
    return pinnedFile('gemma-4-12b-qat-assistant');
}
