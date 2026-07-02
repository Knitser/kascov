"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.getAudioSampleStartFrameAtTimelineZero = void 0;
/**
 * Returns the first frame index to include when building a timeline-aligned waveform.
 * Returns null when the entire sample ends before timeline t=0.
 */
const getAudioSampleStartFrameAtTimelineZero = (sample) => {
    if (sample.timestamp + sample.duration <= 0) {
        return null;
    }
    if (sample.timestamp >= 0) {
        return 0;
    }
    return Math.min(sample.numberOfFrames, Math.ceil(-sample.timestamp * sample.sampleRate));
};
exports.getAudioSampleStartFrameAtTimelineZero = getAudioSampleStartFrameAtTimelineZero;
