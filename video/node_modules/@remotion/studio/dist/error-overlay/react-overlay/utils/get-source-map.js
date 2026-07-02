"use strict";
/*
    Source code adapted from https://github.com/facebook/create-react-app/tree/main/packages/react-error-overlay and refactored in Typescript. This file is MIT-licensed.
*/
Object.defineProperty(exports, "__esModule", { value: true });
exports.getOriginalPosition = void 0;
exports.getSourceMap = getSourceMap;
const trace_mapping_1 = require("@jridgewell/trace-mapping");
const getOriginalPosition = (sourceMap, line, column) => {
    const result = (0, trace_mapping_1.originalPositionFor)(sourceMap, {
        line,
        column,
    });
    return { line: result.line, column: result.column, source: result.source };
};
exports.getOriginalPosition = getOriginalPosition;
function extractSourceMapUrl(fileContents) {
    const regex = /\/\/[#@] ?sourceMappingURL=([^\s'"]+)\s*$/gm;
    let match = null;
    for (;;) {
        const next = regex.exec(fileContents);
        if (next == null) {
            break;
        }
        match = next;
    }
    if (!(match === null || match === void 0 ? void 0 : match[1])) {
        return null;
    }
    return match[1].toString();
}
// TODO: Can import this from BrowserSafeApis.getSourceMapRemotely
async function getSourceMap(fileUri, fileContents) {
    const sm = extractSourceMapUrl(fileContents);
    if (sm === null) {
        return null;
    }
    if (sm.indexOf('data:') === 0) {
        const base64 = /^data:application\/json;([\w=:"-]+;)*base64,/;
        const match2 = sm.match(base64);
        if (!match2) {
            throw new Error('Sorry, non-base64 inline source-map encoding is not supported.');
        }
        const converted = window.atob(sm.substring(match2[0].length));
        return new trace_mapping_1.TraceMap(JSON.parse(converted));
    }
    const index = fileUri.lastIndexOf('/');
    const url = fileUri.substring(0, index + 1) + sm;
    const obj = await fetch(url).then((res) => res.json());
    return new trace_mapping_1.TraceMap(obj);
}
