"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.groupByBackend = void 0;
const groupByBackend = (effects) => {
    const runs = [];
    let current = [];
    let currentBackend = null;
    for (const eff of effects) {
        const { backend } = eff.definition;
        if (currentBackend === null || backend === currentBackend) {
            current.push(eff);
            currentBackend = backend;
        }
        else {
            runs.push({ backend: currentBackend, effects: current });
            current = [eff];
            currentBackend = backend;
        }
    }
    if (currentBackend !== null && current.length > 0) {
        runs.push({ backend: currentBackend, effects: current });
    }
    return runs;
};
exports.groupByBackend = groupByBackend;
