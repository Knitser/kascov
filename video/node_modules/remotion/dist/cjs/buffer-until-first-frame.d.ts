export declare const useBufferUntilFirstFrame: ({ mediaRef, mediaType, onVariableFpsVideoDetected, pauseWhenBuffering, logLevel, mountTime, }: {
    mediaRef: import("react").RefObject<HTMLAudioElement | HTMLVideoElement | null>;
    mediaType: "audio" | "video";
    onVariableFpsVideoDetected: () => void;
    pauseWhenBuffering: boolean;
    logLevel: "error" | "info" | "trace" | "verbose" | "warn";
    mountTime: number | null;
}) => {
    isBuffering: () => boolean;
    bufferUntilFirstFrame: (requestedTime: number) => void;
};
