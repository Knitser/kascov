import type { ForwardRefExoticComponent, RefAttributes } from 'react';
import React from 'react';
export declare const AudioForRendering: ForwardRefExoticComponent<import("./props.js").NativeAudioProps & {
    name?: string | undefined;
    volume?: import("../volume-prop.js").VolumeProp | undefined;
    playbackRate?: number | undefined;
    preservePitch?: boolean | undefined;
    acceptableTimeShiftInSeconds?: number | undefined;
    allowAmplificationDuringRender?: boolean | undefined;
    _remotionInternalNeedsDurationCalculation?: boolean | undefined;
    _remotionInternalNativeLoopPassed?: boolean | undefined;
    toneFrequency?: number | undefined;
    useWebAudioApi?: boolean | undefined;
    pauseWhenBuffering?: boolean | undefined;
    showInTimeline?: boolean | undefined;
    delayRenderTimeoutInMilliseconds?: number | undefined;
    delayRenderRetries?: number | undefined;
    loopVolumeCurveBehavior?: import("./use-audio-frame.js").LoopVolumeCurveBehavior | undefined;
    onError?: ((err: Error) => void) | undefined;
    audioStreamIndex?: number | undefined;
} & {
    readonly onDuration: (src: string, durationInSeconds: number) => void;
    readonly onNativeError: React.ReactEventHandler<HTMLAudioElement>;
} & RefAttributes<HTMLAudioElement>>;
