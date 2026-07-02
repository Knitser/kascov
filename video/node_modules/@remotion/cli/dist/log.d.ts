export declare const Log: {
    formatLogs: (logLevel: "error" | "info" | "trace" | "verbose" | "warn", options: import("@remotion/renderer").LogOptions & {
        tag?: string | undefined;
    }, args: [message?: any, ...optionalParams: any[]]) => string[];
    trace: (options: import("@remotion/renderer").LogOptions & {
        tag?: string | undefined;
    }, message?: any, ...optionalParams: any[]) => void | boolean;
    verbose: (options: import("@remotion/renderer").LogOptions & {
        tag?: string | undefined;
    }, message?: any, ...optionalParams: any[]) => void | boolean;
    info: (options: import("@remotion/renderer").LogOptions, message?: any, ...optionalParams: any[]) => void | boolean;
    warn: (options: import("@remotion/renderer").LogOptions, message?: any, ...optionalParams: any[]) => void | boolean;
    error: (options: import("@remotion/renderer").LogOptions & {
        tag?: string | undefined;
    }, message?: any, ...optionalParams: any[]) => void | boolean;
};
