export declare const cleanupBeforeQuit: ({ indent, logLevel, }: {
    indent: boolean;
    logLevel: "error" | "info" | "trace" | "verbose" | "warn";
}) => void;
export declare const registerCleanupJob: (label: string, job: () => void) => void;
export declare const handleCtrlC: ({ indent, logLevel, }: {
    indent: boolean;
    logLevel: "error" | "info" | "trace" | "verbose" | "warn";
}) => void;
