export type AudioSampleTiming = {
    readonly timestamp: number;
    readonly numberOfFrames: number;
    readonly duration: number;
    readonly sampleRate: number;
};
/**
 * Returns the first frame index to include when building a timeline-aligned waveform.
 * Returns null when the entire sample ends before timeline t=0.
 */
export declare const getAudioSampleStartFrameAtTimelineZero: (sample: AudioSampleTiming) => number | null;
