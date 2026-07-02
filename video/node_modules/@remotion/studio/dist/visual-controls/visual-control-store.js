"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.visualControlStore = void 0;
let version = 0;
const listeners = new Set();
exports.visualControlStore = {
    subscribe(listener) {
        listeners.add(listener);
        return () => {
            listeners.delete(listener);
        };
    },
    getSnapshot() {
        return version;
    },
    emitChange() {
        version++;
        for (const listener of listeners) {
            listener();
        }
    },
};
