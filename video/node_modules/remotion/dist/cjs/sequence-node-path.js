"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.OverrideIdsToNodePathsSettersContext = exports.OverrideIdsToNodePathsGettersContext = void 0;
const react_1 = require("react");
exports.OverrideIdsToNodePathsGettersContext = (0, react_1.createContext)({
    overrideIdToNodePathMappings: {},
});
exports.OverrideIdsToNodePathsSettersContext = (0, react_1.createContext)({
    setOverrideIdToNodePath: () => {
        throw new Error('OverrideIdsToNodePathsSettersContext not initialized');
    },
});
