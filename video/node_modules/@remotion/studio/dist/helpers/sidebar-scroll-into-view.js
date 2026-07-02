"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.maybeScrollAssetSidebarRowIntoView = exports.markAssetSidebarScrollFromRowClick = exports.maybeScrollCompositionSidebarRowIntoView = exports.markCompositionSidebarScrollFromRowClick = exports.scrollSidebarRowIntoViewIfNeeded = void 0;
const getComputedOverflowY = (el) => {
    return getComputedStyle(el).overflowY;
};
const findVerticalScrollParent = (element) => {
    var _a;
    let cur = (_a = element === null || element === void 0 ? void 0 : element.parentElement) !== null && _a !== void 0 ? _a : null;
    while (cur) {
        const overflowY = getComputedOverflowY(cur);
        if (overflowY === 'auto' ||
            overflowY === 'scroll' ||
            overflowY === 'overlay') {
            return cur;
        }
        cur = cur.parentElement;
    }
    return null;
};
const isVerticallyFullyVisibleInSidebarList = (element) => {
    const scrollParent = findVerticalScrollParent(element);
    const rect = element.getBoundingClientRect();
    if (!scrollParent) {
        return rect.top >= 0 && rect.bottom <= window.innerHeight;
    }
    const parentRect = scrollParent.getBoundingClientRect();
    return rect.top >= parentRect.top && rect.bottom <= parentRect.bottom;
};
const scrollSidebarRowIntoViewIfNeeded = (element) => {
    if (!element) {
        return;
    }
    if (isVerticallyFullyVisibleInSidebarList(element)) {
        return;
    }
    element.scrollIntoView({ block: 'nearest', behavior: 'auto' });
};
exports.scrollSidebarRowIntoViewIfNeeded = scrollSidebarRowIntoViewIfNeeded;
let skipCompositionScrollIntoViewForId = null;
const markCompositionSidebarScrollFromRowClick = (compositionId) => {
    skipCompositionScrollIntoViewForId = compositionId;
};
exports.markCompositionSidebarScrollFromRowClick = markCompositionSidebarScrollFromRowClick;
const shouldSkipCompositionScrollIntoView = (compositionId) => {
    if (skipCompositionScrollIntoViewForId === compositionId) {
        skipCompositionScrollIntoViewForId = null;
        return true;
    }
    return false;
};
const maybeScrollCompositionSidebarRowIntoView = ({ element, compositionId, selected, }) => {
    if (!selected) {
        return;
    }
    if (shouldSkipCompositionScrollIntoView(compositionId)) {
        return;
    }
    (0, exports.scrollSidebarRowIntoViewIfNeeded)(element);
};
exports.maybeScrollCompositionSidebarRowIntoView = maybeScrollCompositionSidebarRowIntoView;
let skipAssetScrollIntoViewForPath = null;
const markAssetSidebarScrollFromRowClick = (assetPath) => {
    skipAssetScrollIntoViewForPath = assetPath;
};
exports.markAssetSidebarScrollFromRowClick = markAssetSidebarScrollFromRowClick;
const shouldSkipAssetScrollIntoView = (assetPath) => {
    if (skipAssetScrollIntoViewForPath === assetPath) {
        skipAssetScrollIntoViewForPath = null;
        return true;
    }
    return false;
};
const maybeScrollAssetSidebarRowIntoView = ({ element, assetPath, selected, }) => {
    if (!selected) {
        return;
    }
    if (shouldSkipAssetScrollIntoView(assetPath)) {
        return;
    }
    (0, exports.scrollSidebarRowIntoViewIfNeeded)(element);
};
exports.maybeScrollAssetSidebarRowIntoView = maybeScrollAssetSidebarRowIntoView;
