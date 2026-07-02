import type { OnVideoFrame } from './props';
export declare const useEmitVideoFrame: ({ ref, onVideoFrame, }: {
    ref: import("react").RefObject<HTMLVideoElement | null>;
    onVideoFrame: OnVideoFrame | null;
}) => void;
