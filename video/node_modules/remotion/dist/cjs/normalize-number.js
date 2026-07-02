"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.normalizeNumber = void 0;
const normalizeNumber = (value) => {
    return Math.round(value * 1000000) / 1000000;
};
exports.normalizeNumber = normalizeNumber;
