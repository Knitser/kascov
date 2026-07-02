export declare const playbackLogging: ({ logLevel, tag, message, mountTime, }: {
    logLevel: "error" | "info" | "trace" | "verbose" | "warn";
    tag: string;
    message: string;
    mountTime: number | null;
}) => void;
