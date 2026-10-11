import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, rmSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join, resolve } from 'node:path';

const root = fileURLToPath(new URL('..', import.meta.url));
const crateDir = join(root, 'crates', 'horary-ai-core');
const targets = {
    web: {
        wasmTarget: 'web',
        outDir: '../../src/generated/horary_ai_core_web',
    },
    node: {
        wasmTarget: 'nodejs',
        outDir: '../../src/generated/horary_ai_core_node',
    },
};

const requested = process.argv[2] || 'all';
const selected = requested === 'all' ? Object.keys(targets) : [requested];

for (const target of selected) {
    if (!targets[target]) {
        console.error(`Unknown AI core build target: ${target}`);
        process.exit(1);
    }
}

for (const target of selected) {
    buildTarget(target, targets[target]);
}

function buildTarget(name, config) {
    const output = resolve(crateDir, config.outDir);
    if (name === 'web') {
        const runtime = join(root, 'public', 'litert');
        mkdirSync(runtime, { recursive: true });
        cpSync(join(root, 'node_modules', '@litert-lm', 'core', 'wasm'), runtime, { recursive: true });
    }
    rmSync(output, { recursive: true, force: true });
    const rustEnv = rustupToolchainEnv();
    const wasmPack = wasmPackBin();

    const result = spawnSync(
        wasmPack,
        [
            'build',
            '--release',
            '--target',
            config.wasmTarget,
            '--out-dir',
            config.outDir,
            crateDir,
            '--features',
            'wasm',
        ],
        {
            cwd: root,
            env: rustEnv,
            stdio: 'inherit',
        },
    );

    if (result.status !== 0) {
        process.exit(result.status ?? 1);
    }
    console.log(`Built horary AI core WASM target ${name}: ${output}`);
}

function wasmPackBin() {
    const home = process.env.HOME || process.env.USERPROFILE;
    if (home) {
        const installed = join(home, '.cargo', 'bin', process.platform === 'win32' ? 'wasm-pack.exe' : 'wasm-pack');
        if (existsSync(installed)) return installed;
    }
    return process.platform === 'win32' ? 'wasm-pack.cmd' : 'wasm-pack';
}

function rustupToolchainEnv() {
    const rustc = spawnSync('rustup', ['which', 'rustc'], {
        cwd: root,
        encoding: 'utf8',
    });
    const cargo = spawnSync('rustup', ['which', 'cargo'], {
        cwd: root,
        encoding: 'utf8',
    });
    if (rustc.status !== 0 || cargo.status !== 0) {
        return process.env;
    }

    const rustcPath = rustc.stdout.trim();
    const cargoPath = cargo.stdout.trim();
    const toolchainBin = dirname(cargoPath);
    return {
        ...process.env,
        CARGO: cargoPath,
        RUSTC: rustcPath,
        PATH: `${toolchainBin}${process.platform === 'win32' ? ';' : ':'}${process.env.PATH || ''}`,
    };
}
