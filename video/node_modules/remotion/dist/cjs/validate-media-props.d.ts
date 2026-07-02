import type { VolumeProp } from './volume-prop.js';
export declare const validateMediaProps: (props: {
    volume: VolumeProp | undefined;
    playbackRate: number | undefined;
    preservePitch?: boolean | undefined;
}, component: "Audio" | "Html5Audio" | "Html5Video" | "Video") => void;
