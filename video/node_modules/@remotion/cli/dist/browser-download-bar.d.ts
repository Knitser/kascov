import type { OnBrowserDownload } from '@remotion/renderer';
import type { BrowserDownloadState } from '@remotion/studio-shared';
export declare const defaultBrowserDownloadProgress: ({ indent, logLevel, quiet, onProgress, }: {
    indent: boolean;
    logLevel: "error" | "info" | "trace" | "verbose" | "warn";
    quiet: boolean;
    onProgress: (progress: BrowserDownloadState) => void;
}) => OnBrowserDownload;
