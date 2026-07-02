import type { RenderMediaOnDownload } from '@remotion/renderer';
import type { DownloadProgress } from '@remotion/studio-server';
export declare const makeOnDownload: ({ indent, logLevel, updatesDontOverwrite, downloads, updateRenderProgress, isUsingParallelEncoding, }: {
    indent: boolean;
    logLevel: "error" | "info" | "trace" | "verbose" | "warn";
    updatesDontOverwrite: boolean;
    downloads: DownloadProgress[];
    isUsingParallelEncoding: boolean;
    updateRenderProgress: (progress: {
        newline: boolean;
        printToConsole: boolean;
        isUsingParallelEncoding: boolean;
    }) => void;
}) => RenderMediaOnDownload;
