export declare const getAndValidateAbsoluteOutputFile: (relativeOutputLocation: string, overwrite: boolean, logLevel: "error" | "info" | "trace" | "verbose" | "warn") => string;
export declare const getCliOptions: (options: {
    isStill: boolean;
    logLevel: "error" | "info" | "trace" | "verbose" | "warn";
    indent: boolean;
}) => {
    frameRange: import("@remotion/renderer").FrameRange | null;
    shouldOutputImageSequence: boolean;
    inputProps: Record<string, unknown>;
    envVariables: Record<string, string>;
    stillFrame: number;
    ffmpegOverride: import("@remotion/renderer").FfmpegOverrideFn;
};
