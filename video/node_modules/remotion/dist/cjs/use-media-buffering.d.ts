import type React from 'react';
export declare const useMediaBuffering: ({ element, shouldBuffer, isPremounting, isPostmounting, logLevel, mountTime, src, }: {
    element: React.RefObject<HTMLAudioElement | HTMLVideoElement | null>;
    shouldBuffer: boolean;
    isPremounting: boolean;
    isPostmounting: boolean;
    logLevel: "error" | "info" | "trace" | "verbose" | "warn";
    mountTime: number;
    src: string | null;
}) => boolean;
