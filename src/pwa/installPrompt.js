export function setupPwaInstallPrompt({
    windowRef = globalThis.window,
    documentRef = globalThis.document,
    banner = documentRef?.getElementById?.('installBanner'),
    installButton = documentRef?.getElementById?.('installBtn'),
    dismissButton = documentRef?.getElementById?.('dismissInstall'),
} = {}) {
    if (!windowRef?.addEventListener) return () => {};

    let deferredInstallPrompt = null;

    const showBanner = () => {
        banner?.classList?.add('show');
    };
    const hideBanner = () => {
        banner?.classList?.remove('show');
    };

    const onBeforeInstallPrompt = event => {
        event.preventDefault?.();
        deferredInstallPrompt = event;
        showBanner();
    };

    const onInstallClick = async () => {
        if (!deferredInstallPrompt) return;
        const promptEvent = deferredInstallPrompt;
        promptEvent.prompt?.();
        await promptEvent.userChoice;
        deferredInstallPrompt = null;
        hideBanner();
    };

    const onDismissClick = () => {
        hideBanner();
    };

    windowRef.addEventListener('beforeinstallprompt', onBeforeInstallPrompt);
    installButton?.addEventListener?.('click', onInstallClick);
    dismissButton?.addEventListener?.('click', onDismissClick);

    return () => {
        windowRef.removeEventListener?.('beforeinstallprompt', onBeforeInstallPrompt);
        installButton?.removeEventListener?.('click', onInstallClick);
        dismissButton?.removeEventListener?.('click', onDismissClick);
    };
}
