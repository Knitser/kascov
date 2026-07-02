export declare const getPreviousKeyframeDisplayFrame: (keyframes: readonly {
    frame: number;
}[], currentDisplayFrame: number, durationInFrames: number) => number | null;
export declare const getNextKeyframeDisplayFrame: (keyframes: readonly {
    frame: number;
}[], currentDisplayFrame: number, durationInFrames: number) => number | null;
export declare const hasKeyframeAtSourceFrame: (keyframes: readonly {
    frame: number;
}[], sourceFrame: number) => boolean;
