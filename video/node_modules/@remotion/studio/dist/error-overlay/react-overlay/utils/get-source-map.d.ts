import { TraceMap } from '@jridgewell/trace-mapping';
export type OriginalPosition = {
    line: number | null;
    column: number | null;
    source: string | null;
};
export type CodePosition = {
    line: number;
    column: number;
    source: string;
};
export declare const getOriginalPosition: (sourceMap: TraceMap, line: number, column: number) => OriginalPosition;
export declare function getSourceMap(fileUri: string, fileContents: string): Promise<TraceMap | null>;
