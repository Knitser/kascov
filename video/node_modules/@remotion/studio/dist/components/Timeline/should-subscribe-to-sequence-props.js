"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.shouldSubscribeToSequenceProps = void 0;
const shouldSubscribeToSequenceProps = (sequence, previewConnected) => {
    return (sequence.showInTimeline &&
        Boolean(sequence.controls) &&
        previewConnected &&
        Boolean(sequence.getStack()));
};
exports.shouldSubscribeToSequenceProps = shouldSubscribeToSequenceProps;
