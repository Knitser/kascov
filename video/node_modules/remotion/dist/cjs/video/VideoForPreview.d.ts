import React from 'react';
import type { OnVideoFrame, RemotionVideoProps } from './props';
type VideoForPreviewProps = Omit<RemotionVideoProps, 'onVideoFrame'> & {
    readonly onlyWarnForMediaSeekingError: boolean;
    readonly onDuration: (src: string, durationInSeconds: number) => void;
    readonly pauseWhenBuffering: boolean;
    readonly _remotionInternalNativeLoopPassed: boolean;
    readonly _remotionInternalStack: string | null;
    readonly showInTimeline: boolean;
    readonly onVideoFrame: null | OnVideoFrame;
    readonly crossOrigin?: '' | 'anonymous' | 'use-credentials';
};
export declare const VideoForPreview: React.ForwardRefExoticComponent<Omit<VideoForPreviewProps, "ref"> & React.RefAttributes<HTMLVideoElement>>;
export {};
