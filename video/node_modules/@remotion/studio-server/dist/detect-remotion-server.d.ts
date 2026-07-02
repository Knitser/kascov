type RemotionDetectionResult = {
    type: 'match';
} | {
    type: 'mismatch';
} | {
    type: 'not-remotion';
};
export declare const detectRemotionServer: ({ port, cwd, hostname, }: {
    port: number;
    cwd: string;
    hostname: string;
}) => Promise<RemotionDetectionResult>;
export {};
