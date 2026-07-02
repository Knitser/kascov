export type RemotionAudioContextState = AudioContextState | 'running-to-suspended' | 'suspended-to-running';
export declare const useSingletonAudioContext: ({ logLevel, latencyHint, audioEnabled, sampleRate, }: {
    logLevel: "error" | "info" | "trace" | "verbose" | "warn";
    latencyHint: AudioContextLatencyCategory;
    audioEnabled: boolean;
    sampleRate: number;
}) => {
    audioContext: AudioContext;
    gainNode: GainNode;
    getState: () => RemotionAudioContextState;
    resume: () => Promise<void>;
    suspend: () => Promise<void>;
} | null;
