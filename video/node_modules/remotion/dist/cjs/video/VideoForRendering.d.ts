import type { ForwardRefExoticComponent, RefAttributes } from 'react';
import type { OnVideoFrame, RemotionVideoProps } from './props';
export declare const VideoForRendering: ForwardRefExoticComponent<Omit<RemotionVideoProps, "onVideoFrame"> & {
    readonly onDuration: (src: string, durationInSeconds: number) => void;
    readonly onVideoFrame: OnVideoFrame | null;
} & RefAttributes<HTMLVideoElement>>;
