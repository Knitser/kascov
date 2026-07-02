"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.serializeScaleValue = exports.parseScaleValue = exports.parseValidScaleValue = void 0;
const normalize_number_js_1 = require("./normalize-number.js");
const defaultScaleValue = [1, 1, 1];
const parseScaleString = (value) => {
    var _a, _b;
    const parts = value.trim().split(/\s+/);
    if (parts.length < 1 || parts.length > 3 || parts[0] === '') {
        return null;
    }
    const parsed = parts.map((part) => Number(part));
    if (!parsed.every((part) => Number.isFinite(part))) {
        return null;
    }
    const x = parsed[0];
    const y = (_a = parsed[1]) !== null && _a !== void 0 ? _a : x;
    const z = (_b = parsed[2]) !== null && _b !== void 0 ? _b : 1;
    return [x, y, z];
};
const parseValidScaleValue = (value) => {
    if (typeof value === 'number') {
        return Number.isFinite(value) ? [value, value, 1] : null;
    }
    if (typeof value === 'string') {
        return parseScaleString(value);
    }
    return null;
};
exports.parseValidScaleValue = parseValidScaleValue;
const parseScaleValue = (value) => {
    var _a;
    return (_a = (0, exports.parseValidScaleValue)(value)) !== null && _a !== void 0 ? _a : defaultScaleValue;
};
exports.parseScaleValue = parseScaleValue;
const serializeScaleValue = ([x, y, z]) => {
    const normalizedX = (0, normalize_number_js_1.normalizeNumber)(x);
    const normalizedY = (0, normalize_number_js_1.normalizeNumber)(y);
    const normalizedZ = (0, normalize_number_js_1.normalizeNumber)(z);
    if (normalizedX === normalizedY && normalizedZ === 1) {
        return normalizedX;
    }
    if (normalizedZ === 1) {
        return `${normalizedX} ${normalizedY}`;
    }
    return `${normalizedX} ${normalizedY} ${normalizedZ}`;
};
exports.serializeScaleValue = serializeScaleValue;
