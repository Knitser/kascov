"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.getRulerGuideHighlight = exports.getEditorGuideColor = exports.isGuidePointerUpAClick = void 0;
const colors_1 = require("./colors");
const GUIDE_CLICK_THRESHOLD_PX = 3;
const isGuidePointerUpAClick = ({ pointerDownPosition, guideId, clientX, clientY, }) => {
    if (pointerDownPosition === null || pointerDownPosition.guideId !== guideId) {
        return false;
    }
    return (Math.abs(clientX - pointerDownPosition.clientX) <=
        GUIDE_CLICK_THRESHOLD_PX &&
        Math.abs(clientY - pointerDownPosition.clientY) <= GUIDE_CLICK_THRESHOLD_PX);
};
exports.isGuidePointerUpAClick = isGuidePointerUpAClick;
const getEditorGuideColor = ({ selected, active, }) => {
    if (selected) {
        return colors_1.BLUE;
    }
    return active ? colors_1.SELECTED_GUIDE : colors_1.UNSELECTED_GUIDE;
};
exports.getEditorGuideColor = getEditorGuideColor;
const findGuide = (guidesList, guideId) => {
    var _a;
    if (guideId === null) {
        return null;
    }
    return (_a = guidesList.find((guide) => guide.id === guideId && guide.show)) !== null && _a !== void 0 ? _a : null;
};
const getRulerGuideHighlight = ({ guidesList, selectedItems, hoveredGuideId, draggingGuideId, }) => {
    var _a;
    var _b;
    const selectedGuideId = selectedItems.length === 1 && ((_a = selectedItems[0]) === null || _a === void 0 ? void 0 : _a.type) === 'guide'
        ? ((_b = selectedItems[0].guideId) !== null && _b !== void 0 ? _b : null)
        : null;
    const selectedGuide = findGuide(guidesList, selectedGuideId);
    if (selectedGuide) {
        return {
            guide: selectedGuide,
            color: colors_1.BLUE,
        };
    }
    const draggingGuide = findGuide(guidesList, draggingGuideId);
    if (draggingGuide) {
        return {
            guide: draggingGuide,
            color: colors_1.SELECTED_GUIDE,
        };
    }
    const hoveredGuide = findGuide(guidesList, hoveredGuideId);
    if (hoveredGuide) {
        return {
            guide: hoveredGuide,
            color: colors_1.SELECTED_GUIDE,
        };
    }
    return null;
};
exports.getRulerGuideHighlight = getRulerGuideHighlight;
