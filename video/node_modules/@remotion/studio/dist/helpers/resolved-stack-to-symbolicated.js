"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.resolvedStackToSymbolicated = void 0;
const resolvedStackToSymbolicated = (location) => {
    if (!(location === null || location === void 0 ? void 0 : location.source)) {
        return null;
    }
    return {
        originalFileName: location.source,
        originalLineNumber: location.line,
        originalColumnNumber: location.column,
        originalFunctionName: null,
        originalScriptCode: null,
    };
};
exports.resolvedStackToSymbolicated = resolvedStackToSymbolicated;
