"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.getPortal = exports.portals = void 0;
const portalIds = [
    'menuportal-0',
    'menuportal-1',
    'menuportal-2',
    'menuportal-3',
    'menuportal-4',
    'menuportal-5',
];
exports.portals = 
// This module is imported by Bun tests that run without a DOM.
typeof document === 'undefined'
    ? []
    : portalIds.map((id) => document.getElementById(id));
const getPortal = (i) => {
    const portal = exports.portals[i];
    if (!portal) {
        throw new Error(`Expected menu portal ${i} to exist`);
    }
    return portal;
};
exports.getPortal = getPortal;
