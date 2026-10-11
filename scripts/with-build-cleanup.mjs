import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const janitor = process.env.HOME && join(process.env.HOME, 'Library/Application Support/RustTargetJanitor/rust-target-janitor');
const command = process.argv.slice(2);
if (!command.length) throw new Error('A development command is required.');
const onlyAiCore = command.some(arg => arg === 'check:ai-core:commands' || arg.endsWith('/build-ai-core.mjs'));
const roots = process.env.CARGO_TARGET_DIR
  ? [resolve(root, process.env.CARGO_TARGET_DIR)]
  : [join(root, 'crates/horary-ai-core/target'), ...(onlyAiCore ? [] : [join(root, 'src-tauri/target')])];
const managed = process.platform === 'darwin' && process.env.RUST_JANITOR_ACTIVE !== '1' && janitor && existsSync(janitor);
const program = managed ? janitor : command[0];
const args = managed ? ['--finish-build', ...roots.flatMap(path => ['--rust-root', path]), '--', ...command] : command.slice(1);
const result = spawnSync(program, args, { cwd: root, stdio: 'inherit', shell: process.platform === 'win32' });
if (result.error) throw result.error;
process.exit(result.status ?? 1);
