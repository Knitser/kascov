"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.getSameSequenceInspectorSelection = exports.isSequenceSectionSelection = void 0;
const TimelineSelection_1 = require("../Timeline/TimelineSelection");
const isSequenceSectionSelection = (selection) => {
    return (selection.type === 'sequence' ||
        selection.type === 'sequence-prop' ||
        selection.type === 'sequence-all-effects' ||
        selection.type === 'sequence-effect' ||
        selection.type === 'sequence-effect-prop');
};
exports.isSequenceSectionSelection = isSequenceSectionSelection;
const isSameSequenceInspectorSelection = (selection) => {
    return (selection.type === 'sequence-prop' ||
        selection.type === 'sequence-effect' ||
        selection.type === 'sequence-effect-prop');
};
const getSameSequenceInspectorSelection = (selections) => {
    const firstSelection = selections[0];
    if (!firstSelection || !isSameSequenceInspectorSelection(firstSelection)) {
        return null;
    }
    const rootSequenceKey = (0, TimelineSelection_1.getTimelineSequenceSelectionKey)(firstSelection.nodePathInfo);
    for (const selection of selections) {
        if (!isSameSequenceInspectorSelection(selection)) {
            return null;
        }
        if ((0, TimelineSelection_1.getTimelineSequenceSelectionKey)(selection.nodePathInfo) !==
            rootSequenceKey) {
            return null;
        }
    }
    return firstSelection;
};
exports.getSameSequenceInspectorSelection = getSameSequenceInspectorSelection;
