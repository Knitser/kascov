"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.stringifySequenceExpandedRowKey = exports.stringifySequenceSubscriptionKey = void 0;
const stringifySequenceSubscriptionKey = (key) => {
    return `${key.absolutePath}:${JSON.stringify(key.nodePath)}:${key.sequenceKeys.join('\0')}:${key.effectKeys.map((keys) => keys.join('\0')).join('\0\0')}`;
};
exports.stringifySequenceSubscriptionKey = stringifySequenceSubscriptionKey;
// deliberately not including effect keys, keeping expanded if changing effects
const stringifySequenceExpandedRowKey = (key) => {
    return `${key.absolutePath}:${JSON.stringify(key.nodePath)}:${key.sequenceKeys.join('\0')}`;
};
exports.stringifySequenceExpandedRowKey = stringifySequenceExpandedRowKey;
