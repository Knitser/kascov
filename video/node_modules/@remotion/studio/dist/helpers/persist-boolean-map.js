"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.persistBooleanMap = exports.loadPersistedBooleanMap = exports.toggleBooleanMapKey = exports.onlyExpandedBooleanMapValues = void 0;
const onlyExpandedBooleanMapValues = (state) => {
    const result = {};
    for (const [key, value] of Object.entries(state)) {
        if (value) {
            result[key] = true;
        }
    }
    return result;
};
exports.onlyExpandedBooleanMapValues = onlyExpandedBooleanMapValues;
const toggleBooleanMapKey = (state, key) => {
    const next = { ...state };
    if (next[key]) {
        delete next[key];
    }
    else {
        next[key] = true;
    }
    return next;
};
exports.toggleBooleanMapKey = toggleBooleanMapKey;
const loadPersistedBooleanMap = (sessionStorageKey) => {
    if (typeof window === 'undefined') {
        return {};
    }
    try {
        const raw = window.sessionStorage.getItem(sessionStorageKey);
        if (raw === null) {
            return {};
        }
        const parsed = JSON.parse(raw);
        if (!parsed || typeof parsed !== 'object') {
            return {};
        }
        return (0, exports.onlyExpandedBooleanMapValues)(parsed);
    }
    catch (_a) {
        return {};
    }
};
exports.loadPersistedBooleanMap = loadPersistedBooleanMap;
const persistBooleanMap = (sessionStorageKey, state) => {
    if (typeof window === 'undefined') {
        return;
    }
    try {
        window.sessionStorage.setItem(sessionStorageKey, JSON.stringify((0, exports.onlyExpandedBooleanMapValues)(state)));
    }
    catch (_a) {
        // Ignore quota errors or disabled storage.
    }
};
exports.persistBooleanMap = persistBooleanMap;
