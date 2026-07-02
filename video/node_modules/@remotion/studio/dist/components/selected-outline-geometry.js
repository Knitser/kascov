"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.mixPoint = exports.mix = exports.clamp = void 0;
const clamp = (value, min, max) => {
    return Math.min(max, Math.max(min, value));
};
exports.clamp = clamp;
const mix = (from, to, progress) => {
    return from + (to - from) * progress;
};
exports.mix = mix;
const mixPoint = (from, to, progress) => {
    return {
        x: (0, exports.mix)(from.x, to.x, progress),
        y: (0, exports.mix)(from.y, to.y, progress),
    };
};
exports.mixPoint = mixPoint;
