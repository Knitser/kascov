import type { CompletedClientRender } from '@remotion/studio-shared';
export declare const getCompletedClientRenders: () => CompletedClientRender[];
export declare const addCompletedClientRender: ({ render, remotionRoot, }: {
    render: CompletedClientRender;
    remotionRoot: string;
}) => void;
export declare const removeCompletedClientRender: (id: string) => void;
