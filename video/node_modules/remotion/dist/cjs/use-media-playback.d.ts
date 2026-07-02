import type { RefObject } from 'react';
export declare const useMediaPlayback: ({ mediaRef, src, mediaType, playbackRate: localPlaybackRate, preservePitch, onlyWarnForMediaSeekingError, acceptableTimeshift, pauseWhenBuffering, isPremounting, isPostmounting, onAutoPlayError, }: {
    mediaRef: RefObject<HTMLAudioElement | HTMLVideoElement | null>;
    src: string | undefined;
    mediaType: "audio" | "video";
    playbackRate: number;
    preservePitch: boolean | undefined;
    onlyWarnForMediaSeekingError: boolean;
    acceptableTimeshift: number | null;
    pauseWhenBuffering: boolean;
    isPremounting: boolean;
    isPostmounting: boolean;
    onAutoPlayError: (() => void) | null;
}) => void;
