import type { RefObject } from 'react';
export declare const playAndHandleNotAllowedError: ({ mediaRef, mediaType, onAutoPlayError, logLevel, mountTime, reason, isPlayer, }: {
    mediaRef: RefObject<HTMLAudioElement | HTMLVideoElement | null>;
    mediaType: "audio" | "video";
    onAutoPlayError: (() => void) | null;
    logLevel: "error" | "info" | "trace" | "verbose" | "warn";
    mountTime: number;
    reason: string;
    isPlayer: boolean;
}) => void;
