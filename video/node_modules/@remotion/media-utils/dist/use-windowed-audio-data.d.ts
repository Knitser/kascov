import type { MediaUtilsAudioData } from './types';
export type UseWindowedAudioDataOptions = {
    src: string;
    frame: number;
    fps: number;
    windowInSeconds: number;
    channelIndex?: number;
    /**
     * Captured only from the first render and passed to Mediabunny `UrlSource`.
     * Updates after mount are ignored so hooks do not depend on a new object
     * identity every render (e.g. inline `{credentials: 'include'}`).
     */
    requestInit?: RequestInit;
};
export type UseWindowedAudioDataReturnValue = {
    audioData: MediaUtilsAudioData | null;
    dataOffsetInSeconds: number;
};
export declare const useWindowedAudioData: ({ src, frame, fps, windowInSeconds, channelIndex, requestInit, }: UseWindowedAudioDataOptions) => UseWindowedAudioDataReturnValue;
