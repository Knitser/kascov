import type { SymbolicatedStackFrame } from '../symbolicate-stacktrace';
export declare const makeStackFrameFileName: (frame: SymbolicatedStackFrame) => string;
export declare const formatStackFrameCodeFrame: (frame: SymbolicatedStackFrame) => string | null;
export declare const formatStackFrameLocationLine: (frame: SymbolicatedStackFrame) => string | null;
