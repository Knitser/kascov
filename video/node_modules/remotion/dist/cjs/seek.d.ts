export declare const seek: ({ mediaRef, time, logLevel, why, mountTime, }: {
    mediaRef: HTMLAudioElement | HTMLVideoElement;
    time: number;
    logLevel: "error" | "info" | "trace" | "verbose" | "warn";
    why: string;
    mountTime: number;
}) => number;
