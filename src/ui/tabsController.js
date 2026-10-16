export function setupTabsController({
    documentRef = globalThis.document,
    onActivateTab = () => {},
} = {}) {
    if (!documentRef?.querySelectorAll) {
        return emptyTabsController();
    }

    const tabs = Array.from(documentRef.querySelectorAll('.nav-tab'));
    const panes = Array.from(documentRef.querySelectorAll('.tab-pane'));
    const settingsButton = documentRef.getElementById?.('settingsBtn');
    const listeners = [];

    function activateTab(tabName, { focusTab = false } = {}) {
        const activeTab = tabs.find(tab => tab.dataset?.tab === tabName);
        if (!activeTab) return false;

        for (const tab of tabs) {
            const isActive = tab === activeTab;
            tab.classList?.toggle?.('active', isActive);
            tab.setAttribute?.('aria-selected', String(isActive));
            tab.tabIndex = isActive ? 0 : -1;
        }

        for (const pane of panes) {
            const isActive = pane.id === `tab-${tabName}`;
            pane.classList?.toggle?.('active', isActive);
            pane.hidden = !isActive;
            pane.setAttribute?.('aria-hidden', String(!isActive));
        }

        settingsButton?.setAttribute?.('aria-expanded', String(tabName === 'settings'));
        onActivateTab(tabName);
        if (focusTab) {
            activeTab.focus?.();
        }
        return true;
    }

    function addListener(target, type, listener) {
        target?.addEventListener?.(type, listener);
        listeners.push([target, type, listener]);
    }

    for (const tab of tabs) {
        addListener(tab, 'click', () => {
            activateTab(tab.dataset?.tab);
        });

        addListener(tab, 'keydown', event => {
            const currentIndex = tabs.indexOf(tab);
            let nextIndex = currentIndex;
            if (event.key === 'ArrowRight') nextIndex = (currentIndex + 1) % tabs.length;
            else if (event.key === 'ArrowLeft') nextIndex = (currentIndex - 1 + tabs.length) % tabs.length;
            else if (event.key === 'Home') nextIndex = 0;
            else if (event.key === 'End') nextIndex = tabs.length - 1;
            else return;

            event.preventDefault?.();
            activateTab(tabs[nextIndex].dataset?.tab, { focusTab: true });
        });
    }

    addListener(settingsButton, 'click', () => {
        activateTab('settings', { focusTab: true });
    });

    const currentTab = tabs.find(tab => tab.classList?.contains?.('active')) || tabs[0];
    activateTab(currentTab?.dataset?.tab || 'chart');

    return {
        activateTab,
        cleanup() {
            for (const [target, type, listener] of listeners) {
                target?.removeEventListener?.(type, listener);
            }
            listeners.length = 0;
        },
    };
}

function emptyTabsController() {
    return {
        activateTab() {
            return false;
        },
        cleanup() {},
    };
}
