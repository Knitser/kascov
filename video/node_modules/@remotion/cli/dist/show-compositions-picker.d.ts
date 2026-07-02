export declare const showSingleCompositionsPicker: (validCompositions: import("remotion").VideoConfig[], logLevel: "error" | "info" | "trace" | "verbose" | "warn") => Promise<{
    compositionId: string;
    reason: string;
}>;
export declare const showMultiCompositionsPicker: (validCompositions: import("remotion").VideoConfig[], logLevel: "error" | "info" | "trace" | "verbose" | "warn") => Promise<string[]>;
