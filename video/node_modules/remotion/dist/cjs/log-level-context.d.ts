import type { LogLevel } from './log';
import React = require('react');
export type LoggingContextValue = {
    logLevel: LogLevel | null;
    mountTime: number;
};
export declare const LogLevelContext: React.Context<LoggingContextValue>;
export declare const useLogLevel: () => "error" | "info" | "trace" | "verbose" | "warn";
export declare const useMountTime: () => number;
