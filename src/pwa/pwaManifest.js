export function normalizePwaBase(basePath = '/') {
    const raw = String(basePath || '/').trim();
    if (raw === '/' || raw === '') return '/';

    let normalized = raw;
    if (!normalized.startsWith('/')) normalized = `/${normalized}`;
    if (!normalized.endsWith('/')) normalized = `${normalized}/`;
    return normalized;
}

export function createPwaManifest(basePath = '/') {
    const base = normalizePwaBase(basePath);

    return {
        name: 'Horary — Astrology Calculator',
        short_name: 'Horary',
        description: 'Comprehensive offline astrology calculator with horary, natal charts, and all major house systems',
        theme_color: '#0a0a1a',
        background_color: '#0a0a1a',
        display: 'standalone',
        orientation: 'any',
        start_url: base,
        scope: base,
        icons: [
            { src: 'pwa-192x192.png', sizes: '192x192', type: 'image/png' },
            { src: 'pwa-512x512.png', sizes: '512x512', type: 'image/png' },
            { src: 'pwa-512x512.png', sizes: '512x512', type: 'image/png', purpose: 'maskable' },
        ],
        categories: ['lifestyle', 'utilities'],
    };
}
