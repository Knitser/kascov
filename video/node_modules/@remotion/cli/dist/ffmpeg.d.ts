export declare const dynamicLibEnv: (indent: boolean, logLevel: "error" | "info" | "trace" | "verbose" | "warn", binariesDirectory: string | null) => {
    RUST_BACKTRACE: string;
    DYLD_LIBRARY_PATH: string;
} | {
    RUST_BACKTRACE: string;
    PATH: string;
} | {
    RUST_BACKTRACE: string;
    LD_LIBRARY_PATH: string;
};
export declare const ffmpegCommand: (args: string[], logLevel: "error" | "info" | "trace" | "verbose" | "warn") => never;
export declare const ffprobeCommand: (args: string[], logLevel: "error" | "info" | "trace" | "verbose" | "warn") => never;
