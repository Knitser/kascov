"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.formatMediaDuration = void 0;
const formatMediaDuration = (seconds) => {
    const h = Math.floor(seconds / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    const s = seconds % 60;
    const sFixed = s.toFixed(2).padStart(5, '0');
    if (h > 0) {
        return `${h}:${String(m).padStart(2, '0')}:${sFixed}`;
    }
    return `${String(m).padStart(2, '0')}:${sFixed}`;
};
exports.formatMediaDuration = formatMediaDuration;
