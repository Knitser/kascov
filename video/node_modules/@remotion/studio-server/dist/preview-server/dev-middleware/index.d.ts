import type { webpack } from '@remotion/bundler';
import type { MiddleWare } from './middleware';
export declare const wdm: (compiler: webpack.Compiler, logLevel: "error" | "info" | "trace" | "verbose" | "warn") => MiddleWare;
