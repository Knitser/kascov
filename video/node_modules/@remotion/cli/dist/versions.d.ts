export type VersionAndPath = {
    version: string;
    path: string;
};
export declare const VERSIONS_COMMAND = "versions";
export declare const validateVersionsBeforeCommand: (remotionRoot: string, logLevel: "error" | "info" | "trace" | "verbose" | "warn") => Promise<void>;
export declare const versionsCommand: (remotionRoot: string, logLevel: "error" | "info" | "trace" | "verbose" | "warn") => Promise<void>;
