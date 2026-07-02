import React from 'react';
export declare const useCurrentTimeOfMediaTagWithUpdateTimeStamp: (mediaRef: React.RefObject<HTMLAudioElement | HTMLVideoElement | null>) => React.RefObject<{
    time: number;
    lastUpdate: number;
}>;
