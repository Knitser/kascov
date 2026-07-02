import type { File } from '@babel/types';
import type { RecastCodemod } from '@remotion/studio-shared';
export type Change = {
    description: string;
};
export type ApplyCodeModReturnType = {
    newAst: File;
    changesMade: Change[];
};
export declare const applyCodemod: ({ file, codeMod, }: {
    file: File;
    codeMod: RecastCodemod;
}) => ApplyCodeModReturnType;
