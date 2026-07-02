import type { JobProgressCallback, RenderJob } from '@remotion/studio-server';
export declare const processVideoJob: ({ job, remotionRoot, entryPoint, onProgress, addCleanupCallback, logLevel, }: {
    job: RenderJob;
    remotionRoot: string;
    entryPoint: string;
    onProgress: JobProgressCallback;
    addCleanupCallback: (label: string, cb: () => void) => void;
    logLevel: "error" | "info" | "trace" | "verbose" | "warn";
}) => Promise<void>;
