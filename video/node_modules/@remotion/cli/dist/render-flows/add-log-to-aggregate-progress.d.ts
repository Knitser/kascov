export declare const addLogToAggregateProgress: ({ logs, logLogLevel, logLevel, previewString, tag, }: {
    logs: import("@remotion/studio-shared").BrowserProgressLog[];
    logLogLevel: "error" | "info" | "trace" | "verbose" | "warn";
    logLevel: "error" | "info" | "trace" | "verbose" | "warn";
    previewString: string;
    tag: string | null;
}) => void;
