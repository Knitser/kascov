import type { ErrorWithStackFrame } from '@remotion/renderer';
export declare const printCodeFrameAndStack: ({ symbolicated, logLevel, }: {
    symbolicated: ErrorWithStackFrame;
    logLevel: "error" | "info" | "trace" | "verbose" | "warn";
}) => void;
